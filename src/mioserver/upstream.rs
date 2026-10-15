//! Upstream measurement: this server measures its own internet connection.
//!
//! When nettest runs on a router (or any always-on box at home), the web page
//! measures three legs: device ↔ this server (the normal browser test),
//! this server ↔ internet (this module) and device ↔ internet (the browser
//! against the same public server, as a cross-check).
//!
//! The upstream leg runs the regular measurement client in-process, on its
//! own thread with a current-thread tokio runtime, so the mio workers never
//! block. Only one run at a time, and a minimum pause between runs: every run
//! moves gigabytes over the uplink. The page starts a run with
//! `POST /api/upstream/start` and polls `GET /api/upstream/status`.
//!
//! Shared client state: the in-process client never passes a custom maximum
//! chunk size, so the process-wide `MAX_CHUNK_SIZE` that the server
//! advertises to its own clients stays at its default.

use std::panic::AssertUnwindSafe;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use log::{info, warn};
use serde_json::{json, Value};

use crate::client::api::run_measurement_with_progress;
use crate::client::client::{ClientConfig, Phases, SharedStats};
use crate::client::control_server::servers::{
    fetch_measurement_servers, filter_servers_by_version, MeasurementServer,
};
use crate::client::live::{new_shared_live, LiveState, SharedLive};

/// Default pause between the end of one run and the start of the next.
pub const MIN_INTERVAL: Duration = Duration::from_secs(60);
/// Same connection count as the browser test, so the legs compare fairly.
const THREADS: usize = 3;
/// Shorter jitter / packet-loss phases than the CLI: they are informative
/// only here and often blocked (UDP) towards public servers.
const QOS_PHASE_MS: u64 = 2000;
/// The CLI's default RMBT port, used when `upstream_server` has no port.
const DEFAULT_PORT: u16 = 5005;
/// A run still going after this long is considered hung (e.g. a peer that
/// stopped answering mid-test); its slot is released with an error.
const MAX_RUN: Duration = Duration::from_secs(180);

#[derive(Clone, Debug)]
pub struct UpstreamSettings {
    /// `None`: nearest server from the control server. Else `host[:port]`.
    pub server: Option<String>,
    pub prefer_on_net: bool,
    pub control_server: String,
    pub x_nettest_client: String,
    pub min_interval: Duration,
}

/// The public server measured against; the page uses the same one for the
/// device ↔ internet leg.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Target {
    pub name: String,
    pub host: String,
    /// IPv4 address this server connected to. The page connects the browser
    /// to the same address, so both internet legs use one path and family.
    pub ip: Option<String>,
    /// RMBT port used by this server's client.
    pub port: u16,
    /// WebSocket ports for the browser (`ws://` and `wss://`).
    pub ws_port: Option<u16>,
    pub wss_port: Option<u16>,
    pub on_net: bool,
    pub city: Option<String>,
    pub provider: Option<String>,
}

impl Target {
    fn to_json(&self) -> Value {
        json!({
            "name": self.name,
            "host": self.host,
            "ip": self.ip,
            "port": self.port,
            "wsPort": self.ws_port,
            "wssPort": self.wss_port,
            "onNet": self.on_net,
            "city": self.city,
            "provider": self.provider,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct UpstreamResult {
    pub download_mbps: Option<f64>,
    pub upload_mbps: Option<f64>,
    pub ping_ms: Option<f64>,
    pub jitter_ms: Option<f64>,
    pub packet_loss_percent: Option<f64>,
    pub download_bytes: u64,
    pub upload_bytes: u64,
    pub threads: usize,
    pub failed_threads: usize,
    pub duration_ms: u64,
    pub system: SystemLoad,
    /// Our public address as the target saw it (`/api/ip`; newer servers).
    pub public_ip: Option<String>,
}

impl UpstreamResult {
    fn to_json(&self) -> Value {
        json!({
            "downloadMbps": self.download_mbps,
            "uploadMbps": self.upload_mbps,
            "pingMs": self.ping_ms,
            "jitterMs": self.jitter_ms,
            "packetLossPercent": self.packet_loss_percent,
            "downloadBytes": self.download_bytes,
            "uploadBytes": self.upload_bytes,
            "threads": self.threads,
            "failedThreads": self.failed_threads,
            "durationMs": self.duration_ms,
            "interface": self.system.interface,
            "otherTrafficBytes": self.system.other_bytes,
            "busy": self.system.busy,
            "cpuPeakPercent": self.system.cpu_peak_percent,
            "cpuLimited": self.system.cpu_limited,
            "publicIp": self.public_ip,
        })
    }
}

/// What else happened on the box during the run (Linux only, else empty).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SystemLoad {
    /// Interface of the default route (the uplink on a router).
    pub interface: Option<String>,
    /// Bytes on that interface that were not ours (other devices, on a router).
    pub other_bytes: Option<u64>,
    /// Enough foreign traffic to have lowered the result.
    pub busy: bool,
    /// Median, over the transfer phases, of the busiest core's load.
    pub cpu_peak_percent: Option<f64>,
    /// A core stayed saturated: the box may not reach its line rate itself.
    pub cpu_limited: bool,
}

/// What a run measures. The page asks for `Speed` first, shows its verdict,
/// then fills in latency with a short `Quality` run against the same server.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunKind {
    /// Every phase, like the CLI.
    Full,
    /// Download and upload only.
    Speed,
    /// Ping, jitter and packet loss only: a few kB, not rate limited.
    Quality,
    /// Only pick the public server (for a page that measures it directly).
    Target,
}

impl RunKind {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "full" => Some(RunKind::Full),
            "speed" => Some(RunKind::Speed),
            "quality" => Some(RunKind::Quality),
            "target" => Some(RunKind::Target),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            RunKind::Full => "full",
            RunKind::Speed => "speed",
            RunKind::Quality => "quality",
            RunKind::Target => "target",
        }
    }

    /// Loads the line with bulk traffic (and so falls under `min_interval`).
    fn transfers(self) -> bool {
        matches!(self, RunKind::Full | RunKind::Speed)
    }

    fn phases(self) -> Phases {
        match self {
            RunKind::Full => Phases::default(),
            RunKind::Speed => Phases::speed(),
            RunKind::Quality | RunKind::Target => Phases::quality(),
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum StartError {
    /// A run is in progress.
    Running,
    /// The previous run ended less than `min_interval` ago.
    TooSoon(Duration),
}

struct Current {
    id: u64,
    kind: RunKind,
    ts_ms: u64,
    started: Instant,
    target: Option<Target>,
    live: SharedLive,
}

#[derive(Clone)]
struct Finished {
    id: u64,
    kind: RunKind,
    ts_ms: u64,
    ended: Instant,
    target: Option<Target>,
    outcome: Result<UpstreamResult, String>,
}

#[derive(Default)]
struct Inner {
    next_id: u64,
    current: Option<Current>,
    /// Most recent run of any kind.
    last: Option<Finished>,
    /// Most recent run that transferred data: the rate limit counts from its
    /// end, and a page that is told "too soon" reuses its result.
    last_transfer: Option<Finished>,
}

pub struct Upstream {
    settings: UpstreamSettings,
    inner: Mutex<Inner>,
}

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl Upstream {
    pub fn new(settings: UpstreamSettings) -> Self {
        Upstream {
            settings,
            inner: Mutex::new(Inner::default()),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        Self::reap(&mut inner);
        inner
    }

    /// Give up on a run that exceeded `MAX_RUN`; a late completion of it is
    /// then ignored because its id no longer matches.
    fn reap(inner: &mut Inner) {
        if let Some(cur) = inner.current.take_if(|c| c.started.elapsed() > MAX_RUN) {
            warn!("upstream run {} timed out", cur.id);
            Self::record(
                inner,
                Finished {
                    id: cur.id,
                    kind: cur.kind,
                    ts_ms: cur.ts_ms,
                    ended: Instant::now(),
                    target: cur.target,
                    outcome: Err("the measurement timed out".into()),
                },
            );
        }
    }

    fn record(inner: &mut Inner, f: Finished) {
        if f.kind.transfers() {
            inner.last_transfer = Some(f.clone());
        }
        inner.last = Some(f);
    }

    /// Reserve the single run slot. Returns the run id and its live state.
    /// Also returns the target of the last successful run, which a quality
    /// run reuses so its latency belongs to the same path as the speeds.
    fn begin(&self, kind: RunKind) -> Result<(u64, SharedLive, Option<Target>), StartError> {
        let mut inner = self.lock();
        if inner.current.is_some() {
            return Err(StartError::Running);
        }
        if kind.transfers() {
            if let Some(wait) = self.retry_after(&inner) {
                return Err(StartError::TooSoon(wait));
            }
        }
        let previous_target = inner
            .last_transfer
            .as_ref()
            .filter(|f| f.outcome.is_ok())
            .and_then(|f| f.target.clone());
        inner.next_id += 1;
        let id = inner.next_id;
        let live = new_shared_live();
        inner.current = Some(Current {
            id,
            kind,
            ts_ms: unix_ms(),
            started: Instant::now(),
            target: None,
            live: live.clone(),
        });
        Ok((id, live, previous_target))
    }

    fn set_target(&self, id: u64, target: Target) {
        let mut inner = self.lock();
        if let Some(cur) = inner.current.as_mut().filter(|c| c.id == id) {
            cur.target = Some(target);
        }
    }

    fn complete(&self, id: u64, outcome: Result<UpstreamResult, String>) {
        let mut inner = self.lock();
        let Some(cur) = inner.current.take_if(|c| c.id == id) else {
            return;
        };
        Self::record(
            &mut inner,
            Finished {
                id,
                kind: cur.kind,
                ts_ms: cur.ts_ms,
                ended: Instant::now(),
                target: cur.target,
                outcome,
            },
        );
    }

    /// Start a run on a dedicated thread. Returns the run id.
    pub fn start(self: &Arc<Self>, kind: RunKind) -> Result<u64, StartError> {
        let (id, live, previous_target) = self.begin(kind)?;
        let me = Arc::clone(self);
        let spawned = std::thread::Builder::new()
            .name("nettest-upstream".into())
            .spawn(move || {
                let settings = me.settings.clone();
                let on_target = |t: Target| me.set_target(id, t);
                let reuse = if kind == RunKind::Quality { previous_target } else { None };
                let outcome = std::panic::catch_unwind(AssertUnwindSafe(|| {
                    run_job(&settings, kind, reuse, live, &on_target)
                }))
                .unwrap_or_else(|_| Err("the measurement client crashed".to_string()));
                match &outcome {
                    Ok(r) => info!(
                        "upstream {} run {}: down {:?} Mbit/s, up {:?} Mbit/s, ping {:?} ms",
                        kind.as_str(), id, r.download_mbps, r.upload_mbps, r.ping_ms
                    ),
                    Err(e) => warn!("upstream run {} failed: {}", id, e),
                }
                me.complete(id, outcome);
            });
        if let Err(e) = spawned {
            self.complete(id, Err(format!("could not start the measurement thread: {}", e)));
        }
        Ok(id)
    }

    /// Live state of the current run plus the last finished one.
    pub fn status_json(&self) -> Value {
        let inner = self.lock();
        let mut v = json!({
            "enabled": true,
            "running": inner.current.is_some(),
            "last": inner.last.as_ref().map(finished_json),
            "retryAfterS": self.retry_after(&inner).map(|d| d.as_secs_f64().ceil() as u64),
        });
        if let Some(cur) = &inner.current {
            let snapshot = cur.live.lock().map(|l| l.clone()).unwrap_or_default();
            let mut run = live_json(&snapshot);
            run["id"] = json!(cur.id);
            run["kind"] = json!(cur.kind.as_str());
            run["ts"] = json!(cur.ts_ms);
            run["runningMs"] = json!(cur.started.elapsed().as_millis() as u64);
            run["target"] = cur.target.as_ref().map(Target::to_json).into();
            v["run"] = run;
        }
        v
    }

    /// The last run that measured speeds (what a page told "too soon" reuses).
    pub fn last_json(&self) -> Value {
        let inner = self.lock();
        json!({ "last": inner.last_transfer.as_ref().map(finished_json) })
    }

    fn retry_after(&self, inner: &Inner) -> Option<Duration> {
        let since = inner.last_transfer.as_ref()?.ended.elapsed();
        self.settings.min_interval.checked_sub(since).filter(|d| !d.is_zero())
    }
}

fn finished_json(f: &Finished) -> Value {
    let (result, error) = match &f.outcome {
        Ok(r) => (r.to_json(), Value::Null),
        Err(e) => (Value::Null, json!(e)),
    };
    json!({
        "id": f.id,
        "kind": f.kind.as_str(),
        "ts": f.ts_ms,
        "ageS": f.ended.elapsed().as_secs(),
        "target": f.target.as_ref().map(Target::to_json),
        "result": result,
        "error": error,
    })
}

/// Phase, finished values and, during a transfer phase, the current rate.
fn live_json(l: &LiveState) -> Value {
    let mut v = json!({
        "phase": l.phase,
        "pingMs": l.ping_ms,
        "jitterMs": l.jitter_ms,
        "packetLossPercent": l.packet_loss_percent,
        "downloadMbps": l.download_mbps,
        "uploadMbps": l.upload_mbps,
    });
    let threads = match l.phase.as_str() {
        "download" => &l.download_threads,
        "upload" => &l.upload_threads,
        _ => return v,
    };
    let samples: Vec<Vec<(u64, u64)>> = threads
        .iter()
        .map(|t| t.lock().map(|s| s.clone()).unwrap_or_default())
        .collect();
    let (elapsed_ms, mbps) = current_rate(&samples);
    v["elapsedMs"] = json!(elapsed_ms);
    v["mbps"] = json!(mbps);
    v
}

/// Rate over roughly the last second, summed over threads. Each thread has
/// cumulative `(ns since phase start, bytes)` samples.
fn current_rate(threads: &[Vec<(u64, u64)>]) -> (u64, Option<f64>) {
    const WINDOW_NS: u64 = 1_000_000_000;
    let mut elapsed = 0u64;
    let mut total = 0.0;
    let mut any = false;
    for s in threads {
        let Some(&(t1, b1)) = s.last() else { continue };
        elapsed = elapsed.max(t1);
        let (t0, b0) = s
            .iter()
            .rev()
            .find(|(t, _)| t1.saturating_sub(*t) >= WINDOW_NS)
            .copied()
            .unwrap_or((0, 0));
        if t1 > t0 {
            // bits per ns = Gbit/s; × 1000 = Mbit/s.
            total += (b1 - b0) as f64 * 8.0 / (t1 - t0) as f64 * 1000.0;
            any = true;
        }
    }
    (elapsed / 1_000_000, any.then_some(total))
}

// ---------------------------------------------------------------------------
// The measurement itself
// ---------------------------------------------------------------------------

fn run_job(
    settings: &UpstreamSettings,
    kind: RunKind,
    reuse: Option<Target>,
    live: SharedLive,
    on_target: &dyn Fn(Target),
) -> Result<UpstreamResult, String> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("could not create a runtime: {}", e))?;
    rt.block_on(async {
        if let Ok(mut l) = live.lock() {
            l.phase = "select".into();
        }
        let target = match reuse {
            Some(t) => t,
            None => {
                let mut t = resolve_target(settings).await?;
                t.ip = resolve_v4(&t.host, t.port).await;
                t
            }
        };
        info!(
            "upstream {}: measuring against {} ({}:{}, {:?})",
            kind.as_str(), target.name, target.host, target.port, target.ip
        );
        on_target(target.clone());
        if kind == RunKind::Target {
            return Ok(UpstreamResult::default());
        }

        let config = ClientConfig {
            server: Some(target.ip.clone().unwrap_or_else(|| target.host.clone())),
            port: target.port,
            use_tls: false,
            use_websocket: false,
            json_output: true, // keeps the client quiet on stdout
            // Latency needs one connection; the others would only greet.
            thread_count: if kind.transfers() { THREADS } else { 1 },
            phases: kind.phases(),
            jitter_duration_ms: QOS_PHASE_MS,
            packetloss_duration_ms: QOS_PHASE_MS,
            // No interim upload reports (PUTTIMERESULT interval): public
            // servers stall on it, so the live rate is download-only.
            x_nettest_client: settings.x_nettest_client.clone(),
            control_server: settings.control_server.clone(),
            ..ClientConfig::default()
        };

        let started = Instant::now();
        let sampler = sys::Sampler::start(live.clone());
        let stats = Arc::new(Mutex::new(SharedStats::default()));
        let measured = run_measurement_with_progress(config, stats, live).await;
        let load = sampler.stop();
        let m = measured.map_err(|e| format!("{}", e))?;

        if m.failed_threads >= m.num_threads {
            return Err(format!(
                "the measurement with {} ({}:{}) failed on every connection",
                target.name, target.host, target.port
            ));
        }
        let mbps = |bps: f64| (bps > 0.0).then_some(bps / 1e6);
        let mut result = UpstreamResult {
            download_mbps: mbps(m.download_speed_bps),
            upload_mbps: mbps(m.upload_speed_bps),
            ping_ms: m.ping_median_ns.map(|ns| ns as f64 / 1e6),
            jitter_ms: m.jitter_ns.map(|ns| ns as f64 / 1e6),
            packet_loss_percent: m.packet_loss_percent.map(|p| p as f64),
            download_bytes: m.download_bytes,
            upload_bytes: m.upload_bytes,
            threads: m.num_threads,
            failed_threads: m.failed_threads,
            duration_ms: started.elapsed().as_millis() as u64,
            system: SystemLoad::default(),
            public_ip: None,
        };
        // All datagrams lost means UDP is blocked on the way, not a lossy
        // line: report "not measured" instead of 100 %.
        if result.packet_loss_percent.is_some_and(|p| p >= 99.9) {
            result.packet_loss_percent = None;
            result.jitter_ms = None;
        }
        if !kind.transfers() {
            if result.ping_ms.is_none() {
                return Err(format!("no ping answer from {}", target.host));
            }
            return Ok(result);
        }
        if result.download_mbps.is_none() && result.upload_mbps.is_none() {
            return Err(format!("no data transferred with {}", target.host));
        }
        result.system = load.finish(m.download_bytes + m.upload_bytes);
        result.public_ip = public_ip_seen_by(&target).await;
        Ok(result)
    })
}

/// First IPv4 address of `host`. IPv4 because every device behind a home
/// NAT can reach it, so the browser leg can use the very same address.
async fn resolve_v4(host: &str, port: u16) -> Option<String> {
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        return ip.is_ipv4().then(|| ip.to_string());
    }
    tokio::net::lookup_host((host, port))
        .await
        .ok()?
        .find(|a| a.is_ipv4())
        .map(|a| a.ip().to_string())
}

/// Ask the public server which address our traffic came from. Servers
/// without `/api/ip` (older versions) give `None`.
async fn public_ip_seen_by(target: &Target) -> Option<String> {
    let host = target.ip.as_deref().unwrap_or(&target.host);
    let host = if host.contains(':') { format!("[{}]", host) } else { host.to_string() };
    let port = target.ws_port.unwrap_or(target.port);
    let client = reqwest::Client::builder().timeout(Duration::from_secs(3)).build().ok()?;
    let v: Value = client
        .get(format!("http://{}:{}/api/ip", host, port))
        .send()
        .await
        .ok()?
        .json()
        .await
        .ok()?;
    v["ip"]
        .as_str()
        .filter(|s| s.parse::<std::net::IpAddr>().is_ok_and(|ip| !crate::mioserver::topology::is_local_ip(ip)))
        .map(str::to_string)
}

async fn resolve_target(settings: &UpstreamSettings) -> Result<Target, String> {
    if let Some(spec) = &settings.server {
        return parse_target(spec).ok_or_else(|| format!("invalid upstream_server '{}'", spec));
    }
    let servers = fetch_measurement_servers(&settings.x_nettest_client, &settings.control_server)
        .await
        .map_err(|e| format!("could not get the server list from {}: {}", settings.control_server, e))?;
    let servers = filter_servers_by_version(servers, "2.0.0");
    pick_server(servers, settings.prefer_on_net)
        .and_then(|s| target_from(&s))
        .ok_or_else(|| "no public measurement server available".to_string())
}

/// `host`, `host:port`, `[v6]:port` or a bare IPv6 address.
pub fn parse_target(spec: &str) -> Option<Target> {
    let spec = spec.trim();
    if spec.is_empty() {
        return None;
    }
    let (host, port) = if let Ok(sa) = spec.parse::<std::net::SocketAddr>() {
        (sa.ip().to_string(), sa.port())
    } else if spec.parse::<std::net::IpAddr>().is_ok() {
        (spec.to_string(), DEFAULT_PORT)
    } else if let Some((h, p)) = spec.rsplit_once(':') {
        (h.to_string(), p.parse().ok()?)
    } else {
        (spec.to_string(), DEFAULT_PORT)
    };
    if host.is_empty() || port == 0 {
        return None;
    }
    Some(Target {
        name: host.clone(),
        host,
        port,
        ws_port: Some(port),
        wss_port: None,
        ..Target::default()
    })
}

/// Nearest server, among on-net ones first when asked to.
pub fn pick_server(servers: Vec<MeasurementServer>, prefer_on_net: bool) -> Option<MeasurementServer> {
    let nearest = |it: Vec<MeasurementServer>| {
        it.into_iter()
            .min_by(|a, b| a.distance.partial_cmp(&b.distance).unwrap_or(std::cmp::Ordering::Equal))
    };
    if prefer_on_net && servers.iter().any(|s| s.on_net) {
        return nearest(servers.into_iter().filter(|s| s.on_net).collect());
    }
    nearest(servers)
}

fn target_from(s: &MeasurementServer) -> Option<Target> {
    let host = if !s.web_address.is_empty() {
        s.web_address.clone()
    } else {
        s.ip_address.clone().filter(|ip| !ip.is_empty())?
    };
    let port_of = |kind: &str| s.server_type_details.iter().find(|d| d.server_type == kind);
    let as_port = |p: i32| u16::try_from(p).ok().filter(|p| *p > 0);
    let rmbt = port_of("RMBT");
    let ws = port_of("RMBTws").or(rmbt);
    Some(Target {
        name: s.name.clone(),
        host,
        port: rmbt.and_then(|d| as_port(d.port)).unwrap_or(DEFAULT_PORT),
        ws_port: ws.and_then(|d| as_port(d.port)),
        wss_port: ws.and_then(|d| as_port(d.port_ssl)),
        on_net: s.on_net,
        city: Some(s.city.clone()).filter(|c| !c.is_empty()),
        provider: s.provider.as_ref().map(|p| p.name.clone()),
        ip: None,
    })
}

// ---------------------------------------------------------------------------
// Uplink traffic and CPU sampling during a run (Linux /proc; no-op elsewhere)
// ---------------------------------------------------------------------------

mod sys {
    use super::SystemLoad;
    use crate::client::live::SharedLive;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::thread::JoinHandle;
    use std::time::Duration;

    const PERIOD: Duration = Duration::from_millis(500);
    /// A core counts as saturated at this load.
    const CPU_SATURATED: f64 = 90.0;

    pub struct Sampler {
        stop: Arc<AtomicBool>,
        handle: Option<JoinHandle<Samples>>,
    }

    #[derive(Default)]
    pub struct Samples {
        interface: Option<String>,
        iface_bytes: Option<u64>,
        /// Busiest core per interval, during download/upload only.
        busiest: Vec<f64>,
    }

    impl Sampler {
        pub fn start(live: SharedLive) -> Self {
            let stop = Arc::new(AtomicBool::new(false));
            let flag = stop.clone();
            let handle = std::thread::Builder::new()
                .name("nettest-upstream-load".into())
                .spawn(move || sample(flag, live))
                .ok();
            Sampler { stop, handle }
        }

        pub fn stop(mut self) -> Samples {
            self.stop.store(true, Ordering::Relaxed);
            self.handle
                .take()
                .and_then(|h| h.join().ok())
                .unwrap_or_default()
        }
    }

    impl Samples {
        /// Compare the uplink counters with our own transfer volume.
        pub fn finish(self, our_bytes: u64) -> SystemLoad {
            let other = self.iface_bytes.map(|b| b.saturating_sub(our_bytes));
            // Headers and retransmissions add a few percent; above 20 % (and
            // 10 MB) someone else was using the line.
            let busy = other.is_some_and(|o| o > our_bytes / 5 && o > 10_000_000);
            let mut b = self.busiest;
            b.sort_by(|a, c| a.partial_cmp(c).unwrap_or(std::cmp::Ordering::Equal));
            let median = (!b.is_empty()).then(|| b[b.len() / 2]);
            SystemLoad {
                interface: self.interface,
                other_bytes: other,
                busy,
                cpu_peak_percent: median.map(|m| (m * 10.0).round() / 10.0),
                cpu_limited: median.is_some_and(|m| m >= CPU_SATURATED),
            }
        }
    }

    fn transfer_phase(live: &SharedLive) -> bool {
        live.lock()
            .map(|l| l.phase == "download" || l.phase == "upload")
            .unwrap_or(false)
    }

    fn sample(stop: Arc<AtomicBool>, live: SharedLive) -> Samples {
        let interface = default_route_interface();
        let bytes_at_start = interface.as_deref().and_then(interface_bytes);
        let mut out = Samples { interface: interface.clone(), ..Samples::default() };
        let mut prev_cpu = cpu_times();
        while !stop.load(Ordering::Relaxed) {
            std::thread::sleep(PERIOD);
            let cpu = cpu_times();
            if transfer_phase(&live) {
                if let Some(max) = busiest_core(&prev_cpu, &cpu) {
                    out.busiest.push(max);
                }
            }
            prev_cpu = cpu;
        }
        let bytes_at_end = interface.as_deref().and_then(interface_bytes);
        out.iface_bytes = match (bytes_at_start, bytes_at_end) {
            (Some(a), Some(b)) => Some(b.saturating_sub(a)),
            _ => None,
        };
        out
    }

    /// `(busy, total)` jiffies per core from `/proc/stat`.
    fn cpu_times() -> Vec<(u64, u64)> {
        let Ok(text) = std::fs::read_to_string("/proc/stat") else {
            return Vec::new();
        };
        parse_cpu_times(&text)
    }

    pub(super) fn parse_cpu_times(text: &str) -> Vec<(u64, u64)> {
        text.lines()
            .filter(|l| l.starts_with("cpu") && l.as_bytes().get(3).is_some_and(u8::is_ascii_digit))
            .map(|l| {
                let v: Vec<u64> = l.split_whitespace().skip(1).filter_map(|x| x.parse().ok()).collect();
                let total: u64 = v.iter().take(8).sum();
                // idle + iowait
                let idle = v.get(3).copied().unwrap_or(0) + v.get(4).copied().unwrap_or(0);
                (total.saturating_sub(idle), total)
            })
            .collect()
    }

    pub(super) fn busiest_core(a: &[(u64, u64)], b: &[(u64, u64)]) -> Option<f64> {
        a.iter()
            .zip(b)
            .filter_map(|((busy0, tot0), (busy1, tot1))| {
                let total = tot1.checked_sub(*tot0).filter(|t| *t > 0)?;
                Some(busy1.saturating_sub(*busy0) as f64 * 100.0 / total as f64)
            })
            .reduce(f64::max)
    }

    fn default_route_interface() -> Option<String> {
        let text = std::fs::read_to_string("/proc/net/route").ok()?;
        parse_default_route(&text)
    }

    pub(super) fn parse_default_route(text: &str) -> Option<String> {
        text.lines().skip(1).find_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            (f.len() > 2 && f[1] == "00000000").then(|| f[0].to_string())
        })
    }

    /// Received plus sent bytes of `iface` from `/proc/net/dev`.
    fn interface_bytes(iface: &str) -> Option<u64> {
        let text = std::fs::read_to_string("/proc/net/dev").ok()?;
        parse_interface_bytes(&text, iface)
    }

    pub(super) fn parse_interface_bytes(text: &str, iface: &str) -> Option<u64> {
        text.lines().find_map(|l| {
            let (name, rest) = l.split_once(':')?;
            if name.trim() != iface {
                return None;
            }
            let v: Vec<u64> = rest.split_whitespace().filter_map(|x| x.parse().ok()).collect();
            Some(v.first()? + v.get(8)?)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(min_interval: Duration) -> UpstreamSettings {
        UpstreamSettings {
            server: None,
            prefer_on_net: true,
            control_server: "http://127.0.0.1:9".into(),
            x_nettest_client: "nt".into(),
            min_interval,
        }
    }

    #[test]
    fn single_flight_and_min_interval() {
        let up = Upstream::new(settings(Duration::from_secs(60)));
        let (id, _, _) = up.begin(RunKind::Speed).unwrap();
        assert_eq!(up.begin(RunKind::Speed).err(), Some(StartError::Running));
        assert_eq!(up.begin(RunKind::Quality).err(), Some(StartError::Running));
        assert_eq!(up.status_json()["running"], true);
        up.complete(id, Ok(UpstreamResult { download_mbps: Some(90.0), ..Default::default() }));
        assert!(matches!(up.begin(RunKind::Speed), Err(StartError::TooSoon(_))));
        let s = up.status_json();
        assert_eq!(s["running"], false);
        assert_eq!(s["last"]["result"]["downloadMbps"], 90.0);
        assert_eq!(s["last"]["kind"], "speed");
        assert!(s["retryAfterS"].as_u64().unwrap() > 0);

        let up = Upstream::new(settings(Duration::ZERO));
        let (id, _, _) = up.begin(RunKind::Full).unwrap();
        up.complete(id, Err("boom".into()));
        assert_eq!(up.last_json()["last"]["error"], "boom");
        assert!(up.begin(RunKind::Full).is_ok());
    }

    #[test]
    fn quality_runs_right_after_speed_on_the_same_target() {
        let up = Upstream::new(settings(Duration::from_secs(60)));
        let (id, _, prev) = up.begin(RunKind::Speed).unwrap();
        assert!(prev.is_none());
        let target = Target { name: "pub".into(), host: "pub.example".into(), ..Default::default() };
        up.set_target(id, target.clone());
        up.complete(id, Ok(UpstreamResult { download_mbps: Some(90.0), ..Default::default() }));

        // Not rate limited, and it reuses the speed run's server.
        let (qid, _, prev) = up.begin(RunKind::Quality).unwrap();
        assert_eq!(prev, Some(target));
        up.complete(qid, Ok(UpstreamResult { ping_ms: Some(12.0), ..Default::default() }));
        assert_eq!(up.status_json()["last"]["kind"], "quality");
        // "Too soon" answers still carry the speed result, and the limit
        // still counts from the speed run.
        assert_eq!(up.last_json()["last"]["result"]["downloadMbps"], 90.0);
        assert!(matches!(up.begin(RunKind::Speed), Err(StartError::TooSoon(_))));
        assert_eq!(RunKind::parse("quality"), Some(RunKind::Quality));
        assert_eq!(RunKind::parse("x"), None);
        // Picking the target transfers nothing, so it is not rate limited
        // and does not replace the speed result.
        assert_eq!(RunKind::parse("target"), Some(RunKind::Target));
        let (tid, _, _) = up.begin(RunKind::Target).unwrap();
        up.complete(tid, Ok(UpstreamResult::default()));
        assert_eq!(up.status_json()["last"]["kind"], "target");
        assert_eq!(up.last_json()["last"]["result"]["downloadMbps"], 90.0);
    }

    #[test]
    fn stale_completion_is_ignored() {
        let up = Upstream::new(settings(Duration::ZERO));
        let (id, _, _) = up.begin(RunKind::Full).unwrap();
        up.complete(id + 7, Err("other".into()));
        assert_eq!(up.status_json()["running"], true);
    }

    #[test]
    fn target_parsing() {
        assert_eq!(parse_target("example.org").unwrap().port, 5005);
        let t = parse_target("example.org:8080").unwrap();
        assert_eq!((t.host.as_str(), t.port, t.ws_port), ("example.org", 8080, Some(8080)));
        assert_eq!(parse_target("[2001:db8::1]:9000").unwrap().host, "2001:db8::1");
        assert_eq!(parse_target("2001:db8::1").unwrap().port, 5005);
        assert!(parse_target("").is_none());
        assert!(parse_target("host:notaport").is_none());
    }

    #[test]
    fn rate_over_last_second() {
        // 125 MB/s for 2 s on one thread = 1000 Mbit/s.
        let s: Vec<(u64, u64)> = (1..=20).map(|i| (i * 100_000_000, i * 12_500_000)).collect();
        let (ms, mbps) = current_rate(&[s.clone(), s]);
        assert_eq!(ms, 2000);
        assert!((mbps.unwrap() - 2000.0).abs() < 1.0);
        assert_eq!(current_rate(&[vec![]]), (0, None));
    }

    #[test]
    fn proc_parsing() {
        let stat = "cpu  10 0 10 80 0 0 0 0 0 0\ncpu0 10 0 0 90 0 0 0 0 0 0\ncpu1 50 0 0 50 0 0 0 0 0 0\nintr 1\n";
        let a = sys::parse_cpu_times(stat);
        assert_eq!(a, vec![(10, 100), (50, 100)]);
        let b = vec![(20, 200), (145, 200)];
        assert_eq!(sys::busiest_core(&a, &b), Some(95.0));

        let route = "Iface\tDestination\tGateway\nlan0\t0000A8C0\t00000000\nwan0\t00000000\t0100A8C0\n";
        assert_eq!(sys::parse_default_route(route).as_deref(), Some("wan0"));

        let dev = "Inter-|   Receive\n face |bytes packets\n  wan0: 1000 1 0 0 0 0 0 0 500 2 0 0 0 0 0 0\n";
        assert_eq!(sys::parse_interface_bytes(dev, "wan0"), Some(1500));
    }

    #[test]
    fn busy_and_cpu_limited() {
        let s = sys::Samples::default();
        let load = s.finish(100);
        assert!(!load.busy && !load.cpu_limited && load.cpu_peak_percent.is_none());
    }
}
