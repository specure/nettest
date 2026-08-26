//! Browser WASM driver — the full RMBT client over browser WebSockets.
//!
//! Runs the RMBT flow **in Rust (wasm)** by driving the SAME handler functions
//! the native client uses (`client::handlers::*` via `basic_handler`), over
//! `Stream::Js(JsWss)` WebSockets. A browser has no blocking `poll()`, so the
//! async [`pump_until`] replaces the native mio event loop — it is the analogue
//! of `TestState::process_phase`.
//!
//! Phase order mirrors the native runner (`client::runnner::run_threads`):
//!
//! ```text
//! greeting → pretest (GETCHUNKS) → ping → download (GETTIME)
//!          → upload (PUTTIMERESULT, or PUT in legacy mode)
//!          → QoS (jitter / packet loss over QUIC datagrams) → SIGNEDRESULT
//! ```
//!
//! Every phase except ping runs on all N connections concurrently (the browser
//! analogue of the native thread pool) on the single JS event loop via
//! `join_all`, and per-connection samples are aggregated with the same
//! calculator the native client uses. Each connection carries the whole flow, so
//! one WebSocket is opened per thread — exactly like one native socket per
//! thread.
//!
//! Quality of service: the native jitter / packet-loss phases run over UDP,
//! which a browser has no socket for, so [`qos`] runs them over WebTransport
//! (QUIC datagrams) instead. Where that is unavailable — no WebTransport, the
//! endpoint disabled, UDP blocked — jitter falls back to the variation of the
//! `PING` round trips and packet loss is reported as unmeasured, because TCP
//! hides loss behind retransmissions.

pub mod qos;
pub mod save;

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::io;
use std::rc::Rc;
use std::task::Poll as TaskPoll;
use std::time::Duration;

use futures::future::{join_all, poll_fn, select};
use futures::pin_mut;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_time::Instant;

use crate::client::calculator::{
    calculate_download_speed_from_stats_silent, calculate_upload_speed_from_stats_silent,
    rfc3550_jitter_ns,
};
use crate::client::constants::{get_max_chunk_size, MIN_CHUNK_SIZE};
use crate::client::handlers::basic_handler::{
    handle_client_readable_data, handle_client_writable_data,
};
use crate::client::state::{MeasurementState, TestPhase};
use crate::reactor::{Interest, Poll, Token};
use crate::stream::js_wss::{JsWss, Notify};
use crate::stream::stream::Stream;

/// Parallel connections (browser analogue of the native thread count).
const DEFAULT_THREADS: usize = 3;
/// Nominal download / upload duration, same defaults as `ClientConfig`.
const DEFAULT_DOWNLOAD_MS: u64 = 7000;
const DEFAULT_UPLOAD_MS: u64 = 7000;

const GREETING_TIMEOUT_MS: u64 = 15_000;
const PRETEST_TIMEOUT_MS: u64 = 15_000;
const PING_TIMEOUT_MS: u64 = 10_000;
const SIGNED_RESULT_TIMEOUT_MS: u64 = 12_000;
/// Slack added on top of a phase's nominal duration before the pump gives up.
const PHASE_SLACK_MS: u64 = 10_000;
/// How long the pump waits for readability before re-checking its deadline.
const READ_TICK_MS: i32 = 100;
/// Fallback wait for the browser to flush its send buffer, used once the cheap
/// macrotask yields below stop helping (i.e. the link, not the client, is the
/// bottleneck).
const WRITE_TICK_MS: i32 = 1;
/// How many `yield_now()` turns with a *completely undrained* send buffer to
/// spend before falling back to [`WRITE_TICK_MS`]. Each yield costs a few
/// microseconds, so this spins while the browser is actually flushing and hands
/// over to timers when the link — not the client — is the bottleneck.
const MAX_SPIN_YIELDS: u32 = 64;
/// Throttle for the JS progress callback.
const PROGRESS_EVERY_MS: u64 = 150;
/// Quiet time between the upload and the QoS phase, so the queues a saturated
/// link just filled are drained before jitter and loss are measured.
const QOS_SETTLE_MS: i32 = 750;

// ---------------------------------------------------------------------------
// JS helpers
// ---------------------------------------------------------------------------

fn err_str(v: &JsValue) -> String {
    v.as_string().unwrap_or_else(|| format!("{v:?}"))
}

fn js_err(msg: impl AsRef<str>) -> JsValue {
    JsValue::from_str(msg.as_ref())
}

/// `setTimeout` off the global object, so this works in a window, a worker and
/// in Node (where `web_sys::window()` is absent).
fn global_set_timeout(cb: &JsValue, ms: i32) -> Result<(), JsValue> {
    let global = js_sys::global();
    let f: js_sys::Function = js_sys::Reflect::get(&global, &JsValue::from_str("setTimeout"))?
        .dyn_into()
        .map_err(|_| js_err("setTimeout is not a function"))?;
    f.call2(&global, cb, &JsValue::from_f64(ms as f64))?;
    Ok(())
}

/// Yield to the JS event loop for `ms` (a macrotask, so the browser gets to run
/// its network callbacks — a microtask would not).
async fn sleep_ms(ms: i32) {
    let promise = js_sys::Promise::new(&mut |resolve, _reject| {
        if global_set_timeout(&resolve, ms).is_err() {
            let _ = resolve.call0(&JsValue::NULL);
        }
    });
    let _ = JsFuture::from(promise).await;
}

/// A `MessageChannel` used as an unclamped macrotask source.
///
/// `setTimeout` is the obvious way to hand control back to the browser, but
/// once timers nest it is clamped to ~4 ms (measured: ~226 turns/s in Chrome vs
/// ~390 000/s for a channel message). The upload loop yields once per full send
/// buffer, so that clamp alone would cap the measured upload speed at roughly
/// `SEND_HIGH_WATER × 226/s` per connection — a client-side ceiling, not the
/// link's. A channel message is a real macrotask, so the browser still gets to
/// run its network callbacks between turns.
struct Yielder {
    channel: web_sys::MessageChannel,
    waiting: Rc<RefCell<VecDeque<js_sys::Function>>>,
    _onmessage: Closure<dyn FnMut(web_sys::MessageEvent)>,
}

impl Yielder {
    fn new() -> Result<Yielder, JsValue> {
        let channel = web_sys::MessageChannel::new()?;
        let waiting: Rc<RefCell<VecDeque<js_sys::Function>>> = Rc::new(RefCell::new(VecDeque::new()));
        let queue = waiting.clone();
        let onmessage = Closure::wrap(Box::new(move |_e: web_sys::MessageEvent| {
            let next = queue.borrow_mut().pop_front();
            if let Some(resolve) = next {
                let _ = resolve.call0(&JsValue::NULL);
            }
        }) as Box<dyn FnMut(web_sys::MessageEvent)>);
        channel.port1().set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
        Ok(Yielder { channel, waiting, _onmessage: onmessage })
    }

    fn schedule(&self, resolve: js_sys::Function) {
        self.waiting.borrow_mut().push_back(resolve);
        let _ = self.channel.port2().post_message(&JsValue::NULL);
    }
}

thread_local! {
    static YIELDER: Option<Yielder> = Yielder::new().ok();
}

/// Hand control back to the JS event loop for exactly one macrotask.
async fn yield_now() {
    let promise = js_sys::Promise::new(&mut |resolve, _reject| {
        let scheduled = YIELDER.with(|y| match y {
            Some(y) => {
                y.schedule(resolve.clone());
                true
            }
            None => false,
        });
        if !scheduled && global_set_timeout(&resolve, 0).is_err() {
            let _ = resolve.call0(&JsValue::NULL);
        }
    });
    let _ = JsFuture::from(promise).await;
}

async fn await_open(n: &Notify) -> Result<(), JsValue> {
    poll_fn(|cx| {
        if n.is_open() {
            TaskPoll::Ready(Ok(()))
        } else if n.is_closed() {
            TaskPoll::Ready(Err(js_err("socket closed before open")))
        } else {
            n.set_waker(cx.waker());
            TaskPoll::Pending
        }
    })
    .await
}

/// Wait until the socket has data (or closed), but no longer than `tick_ms`, so
/// the caller can re-check its phase deadline.
async fn wait_readable_or_tick(n: &Notify, tick_ms: i32) {
    let readable = poll_fn(|cx| {
        if n.has_incoming() || n.is_closed() {
            TaskPoll::Ready(())
        } else {
            n.set_waker(cx.waker());
            TaskPoll::Pending
        }
    });
    let tick = sleep_ms(tick_ms);
    pin_mut!(readable);
    pin_mut!(tick);
    let _ = select(readable, tick).await;
}

// ---------------------------------------------------------------------------
// Options / progress
// ---------------------------------------------------------------------------

fn get_prop(obj: &JsValue, key: &str) -> Option<JsValue> {
    if !obj.is_object() {
        return None;
    }
    js_sys::Reflect::get(obj, &JsValue::from_str(key))
        .ok()
        .filter(|v| !v.is_undefined() && !v.is_null())
}

fn opt_u64(obj: &JsValue, key: &str, default: u64) -> u64 {
    get_prop(obj, key)
        .and_then(|v| v.as_f64())
        .map(|f| f.max(0.0) as u64)
        .unwrap_or(default)
}

fn opt_bool(obj: &JsValue, key: &str, default: bool) -> bool {
    get_prop(obj, key)
        .and_then(|v| v.as_bool())
        .unwrap_or(default)
}

fn opt_fn(obj: &JsValue, key: &str) -> Option<js_sys::Function> {
    get_prop(obj, key).and_then(|v| v.dyn_into::<js_sys::Function>().ok())
}

/// Which byte counter a phase feeds into the progress callback.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Track {
    None,
    Download,
    Upload,
}

/// Shared, single-threaded reporting context for the whole measurement.
struct Ctx {
    log_fn: Option<js_sys::Function>,
    progress_fn: Option<js_sys::Function>,
    phase: RefCell<String>,
    /// Per-connection byte counters for the phase currently running.
    bytes: RefCell<Vec<u64>>,
    phase_start: Cell<Option<Instant>>,
    last_emit: Cell<Option<Instant>>,
}

impl Ctx {
    fn new(threads: usize, log_fn: Option<js_sys::Function>, progress_fn: Option<js_sys::Function>) -> Ctx {
        Ctx {
            log_fn,
            progress_fn,
            phase: RefCell::new("idle".to_string()),
            bytes: RefCell::new(vec![0; threads]),
            phase_start: Cell::new(None),
            last_emit: Cell::new(None),
        }
    }

    fn log(&self, msg: &str) {
        if let Some(f) = &self.log_fn {
            let _ = f.call1(&JsValue::NULL, &JsValue::from_str(msg));
        }
    }

    /// Start a phase: resets the counters and announces it to the callbacks.
    fn begin_phase(&self, name: &str) {
        *self.phase.borrow_mut() = name.to_string();
        for b in self.bytes.borrow_mut().iter_mut() {
            *b = 0;
        }
        self.phase_start.set(Some(Instant::now()));
        self.last_emit.set(None);
        self.emit(true);
    }

    /// Record a connection's byte counter and emit a throttled progress event.
    fn note(&self, idx: usize, bytes: u64, force: bool) {
        if let Some(slot) = self.bytes.borrow_mut().get_mut(idx) {
            *slot = bytes;
        }
        self.emit(force);
    }

    fn emit(&self, force: bool) {
        let f = match &self.progress_fn {
            Some(f) => f,
            None => return,
        };
        let now = Instant::now();
        if !force {
            if let Some(last) = self.last_emit.get() {
                if now.duration_since(last) < Duration::from_millis(PROGRESS_EVERY_MS) {
                    return;
                }
            }
        }
        self.last_emit.set(Some(now));

        let elapsed_ms = self
            .phase_start
            .get()
            .map(|t| now.duration_since(t).as_secs_f64() * 1000.0)
            .unwrap_or(0.0);
        let total: u64 = self.bytes.borrow().iter().sum();
        let mbps = if elapsed_ms > 0.0 {
            (total as f64 * 8.0) / (elapsed_ms / 1000.0) / 1e6
        } else {
            0.0
        };

        let obj = js_sys::Object::new();
        let set = |k: &str, v: JsValue| {
            let _ = js_sys::Reflect::set(&obj, &JsValue::from_str(k), &v);
        };
        set("phase", JsValue::from_str(&self.phase.borrow()));
        set("bytes", JsValue::from_f64(total as f64));
        set("elapsedMs", JsValue::from_f64(elapsed_ms));
        set("mbps", JsValue::from_f64(mbps));
        let _ = f.call1(&JsValue::NULL, &obj);
    }
}

// ---------------------------------------------------------------------------
// Connection + pump
// ---------------------------------------------------------------------------

/// One browser WebSocket carrying the whole RMBT flow (one native thread's worth).
struct Conn {
    idx: usize,
    state: MeasurementState,
    notify: Notify,
    poll: Poll,
}

fn dummy_addr() -> std::net::SocketAddr {
    std::net::SocketAddr::from(([0u8, 0, 0, 0], 0))
}

impl Conn {
    /// Open a socket and wait for the browser's WebSocket handshake.
    async fn connect(url: &str, idx: usize) -> Result<Conn, JsValue> {
        let poll = Poll::new().map_err(|e| js_err(e.to_string()))?;
        let wss = JsWss::connect(url)?;
        let notify = wss.notify();
        let state = MeasurementState::new(Stream::Js(wss), Token(idx), dummy_addr());
        let conn = Conn { idx, state, notify, poll };
        await_open(&conn.notify).await?;
        Ok(conn)
    }

    fn interest(&self) -> Interest {
        match &self.state.stream {
            Stream::Js(s) => s.interest(),
        }
    }

    /// Can the browser accept more bytes right now (socket open, send buffer
    /// below its high-water mark)?
    fn send_ready(&self) -> bool {
        match &self.state.stream {
            Stream::Js(s) => s.is_writable(),
        }
    }

    fn buffered(&self) -> u64 {
        match &self.state.stream {
            Stream::Js(s) => s.buffered_amount() as u64,
        }
    }

    fn set_interest(&mut self, interest: Interest) {
        let token = self.state.token;
        let poll = &self.poll;
        if let Stream::Js(s) = &mut self.state.stream {
            let _ = s.reregister(poll, token, interest);
        }
    }

    fn close(&mut self) {
        let _ = self.state.stream.close();
    }

    /// Bytes the progress callback should report for `track`. For upload the
    /// browser's own send buffer is subtracted: those bytes are queued in the
    /// tab, not yet on the wire.
    fn tracked_bytes(&self, track: Track) -> u64 {
        match track {
            Track::None => 0,
            Track::Download => self.state.bytes_received,
            Track::Upload => self.state.bytes_sent.saturating_sub(self.buffered()),
        }
    }
}

/// Wait for room in the browser's send buffer.
///
/// One cheap macrotask yield per call, so a fast link keeps the socket busy
/// every turn. The spin counter only advances while the buffer does not shrink
/// at all; a link slow enough to leave it untouched for [`MAX_SPIN_YIELDS`]
/// turns gets 1 ms timer waits instead, so a seven-second upload over a modest
/// connection doesn't burn a core.
async fn wait_for_send_room(conn: &Conn, spins: &mut u32) {
    let before = conn.buffered();
    yield_now().await;
    if conn.buffered() < before {
        *spins = 0;
    } else {
        *spins += 1;
        if *spins >= MAX_SPIN_YIELDS {
            sleep_ms(WRITE_TICK_MS).await;
        }
    }
}

/// Async analogue of the native `TestState::process_phase`: drive the shared
/// handlers until `state.phase == target`.
///
/// The differences to the native loop are exactly the two things a browser
/// lacks: readiness comes from `Notify` (fed by `WebSocket.onmessage`) instead
/// of mio, and a `WouldBlock` on the *write* side means the browser's send
/// buffer is full — the pump then yields a macrotask so the browser can flush.
async fn pump_until(
    conn: &mut Conn,
    ctx: &Ctx,
    target: TestPhase,
    timeout_ms: u64,
    track: Track,
) -> Result<(), JsValue> {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    // Consecutive turns spent waiting on a full send buffer, reset by progress.
    let mut spins = 0u32;
    loop {
        if conn.state.phase == target {
            ctx.note(conn.idx, conn.tracked_bytes(track), true);
            return Ok(());
        }
        if conn.state.failed {
            return Err(js_err(format!(
                "thread {} failed in phase {:?}",
                conn.idx, conn.state.phase
            )));
        }
        if Instant::now() >= deadline {
            return Err(js_err(format!(
                "thread {} timed out in phase {:?} (waiting for {:?})",
                conn.idx, conn.state.phase, target
            )));
        }

        let want = conn.interest();
        // Drain pending input first only when the phase asked for both (the
        // upload send phases, where interim TIMERESULT arrives while writing).
        let do_write = want.is_writable() && !(want.is_readable() && conn.notify.has_incoming());

        let sent_before = conn.state.bytes_sent;
        let result = if do_write {
            if !conn.send_ready() {
                // Browser send buffer is full: let the event loop flush it.
                wait_for_send_room(conn, &mut spins).await;
                ctx.note(conn.idx, conn.tracked_bytes(track), false);
                continue;
            }
            handle_client_writable_data(&mut conn.state, &conn.poll)
        } else if conn.notify.has_incoming() {
            handle_client_readable_data(&mut conn.state, &conn.poll)
        } else if conn.notify.is_closed() {
            return Err(js_err(format!(
                "thread {} socket closed in phase {:?}",
                conn.idx, conn.state.phase
            )));
        } else {
            wait_readable_or_tick(&conn.notify, READ_TICK_MS).await;
            ctx.note(conn.idx, conn.tracked_bytes(track), false);
            continue;
        };

        // The upload send handlers loop internally until the buffer is full, so
        // they report `WouldBlock` even on a completely healthy fast link. Bytes
        // actually handed to the socket — not the return value — are what says
        // the spin is still paying off.
        if conn.state.bytes_sent > sent_before {
            spins = 0;
        }
        match result {
            Ok(_) => spins = 0,
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                if do_write {
                    wait_for_send_room(conn, &mut spins).await;
                } else {
                    wait_readable_or_tick(&conn.notify, READ_TICK_MS).await;
                }
            }
            Err(e) => {
                return Err(js_err(format!(
                    "thread {} handler error in phase {:?}: {e}",
                    conn.idx, conn.state.phase
                )))
            }
        }
        ctx.note(conn.idx, conn.tracked_bytes(track), false);
    }
}

/// Send one line on the control channel and wait for the reply line starting
/// with `expect`.
///
/// The RMBT state machine has no phases for the QoS commands, and does not need
/// any: between phases the connection sits at the command prompt, where the
/// exchange is a plain line in, line out. Bypassing the phase machine here keeps
/// the QoS protocol out of a state enum shared with the native client.
async fn command(
    conn: &mut Conn,
    request: &str,
    expect: &str,
    timeout_ms: u64,
) -> Result<String, JsValue> {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    let bytes = request.as_bytes();
    let mut written = 0;
    while written < bytes.len() {
        match conn.state.stream.write(&bytes[written..]) {
            Ok(0) => return Err(js_err("control channel accepted no bytes")),
            Ok(n) => written += n,
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => yield_now().await,
            Err(e) => return Err(js_err(format!("writing {}: {e}", request.trim()))),
        }
        if Instant::now() >= deadline {
            return Err(js_err(format!("timeout sending {}", request.trim())));
        }
    }

    let mut buffer = String::new();
    loop {
        let mut chunk = [0u8; 4096];
        match conn.state.stream.read(&mut chunk) {
            Ok(0) => return Err(js_err("control channel closed")),
            Ok(n) => {
                buffer.push_str(&String::from_utf8_lossy(&chunk[..n]));
                if let Some(line) = buffer.lines().find(|l| l.starts_with(expect)) {
                    return Ok(line.to_string());
                }
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return Err(js_err(format!(
                        "timeout waiting for {expect} after {}",
                        request.trim()
                    )));
                }
                wait_readable_or_tick(&conn.notify, READ_TICK_MS).await;
            }
            Err(e) => return Err(js_err(format!("reading {expect}: {e}"))),
        }
    }
}

// ---------------------------------------------------------------------------
// Phases
// ---------------------------------------------------------------------------

/// RMBT chunk sizes are powers of two between `MIN_CHUNK_SIZE` and the
/// configured maximum — the pre-generated chunk buffers exist only for those, so
/// an explicitly configured size is snapped down to the nearest valid one.
fn snap_chunk_size(requested: usize) -> usize {
    let min = MIN_CHUNK_SIZE as usize;
    let max = (get_max_chunk_size() as usize).max(min);
    let mut size = min;
    while size * 2 <= requested.min(max) {
        size *= 2;
    }
    size
}

async fn run_greeting(conn: &mut Conn, ctx: &Ctx) -> Result<(), JsValue> {
    conn.state.phase_start_time = Some(Instant::now());
    pump_until(conn, ctx, TestPhase::GreetingCompleted, GREETING_TIMEOUT_MS, Track::None).await
}

/// Pre-test (`GETCHUNKS`): doubles the chunk size for ~2 s to pick the chunk
/// size the download and upload phases will use.
async fn run_pretest(conn: &mut Conn, ctx: &Ctx) -> Result<(), JsValue> {
    conn.state.total_chunks = 1;
    conn.state.chunk_size = MIN_CHUNK_SIZE as usize;
    conn.state.chunk_buffer.resize(conn.state.chunk_size, 0);
    conn.state.read_pos = 0;
    conn.state.write_pos = 0;
    conn.state.phase = TestPhase::GetChunksSendChunksCommand;
    conn.set_interest(Interest::WRITABLE);
    conn.state.phase_start_time = Some(Instant::now());
    pump_until(conn, ctx, TestPhase::GetChunksCompleted, PRETEST_TIMEOUT_MS, Track::None).await
}

async fn run_ping(conn: &mut Conn, ctx: &Ctx) -> Result<(), JsValue> {
    conn.state.ping_times.clear();
    conn.state.read_pos = 0;
    conn.state.write_pos = 0;
    conn.state.phase = TestPhase::PingSendPing;
    conn.set_interest(Interest::WRITABLE);
    conn.state.phase_start_time = Some(Instant::now());
    pump_until(conn, ctx, TestPhase::PingCompleted, PING_TIMEOUT_MS, Track::None).await
}

async fn run_download(conn: &mut Conn, ctx: &Ctx, chunk_size: usize, duration_ms: u64) -> Result<(), JsValue> {
    conn.state.chunk_size = chunk_size;
    conn.state.download_duration_ms = duration_ms;
    conn.state.download_measurements.clear();
    conn.state.bytes_received = 0;
    conn.state.read_pos = 0;
    conn.state.write_pos = 0;
    conn.state.phase = TestPhase::GetTimeSendCommand;
    conn.set_interest(Interest::WRITABLE);
    conn.state.phase_start_time = Some(Instant::now());
    pump_until(
        conn,
        ctx,
        TestPhase::GetTimeCompleted,
        duration_ms + PHASE_SLACK_MS,
        Track::Download,
    )
    .await
}

/// Upload: `PUTTIMERESULT` (the server times the transfer and reports the
/// samples), or the legacy `PUT` where every chunk is acknowledged with
/// `TIME <t> BYTES <b>`.
async fn run_upload(
    conn: &mut Conn,
    ctx: &Ctx,
    chunk_size: usize,
    duration_ms: u64,
    legacy: bool,
    interim_interval_ms: u64,
) -> Result<(), JsValue> {
    conn.state.chunk_size = snap_chunk_size(chunk_size);
    conn.state.upload_duration_ms = duration_ms;
    conn.state.puttimeresult_interval_ms = interim_interval_ms;
    conn.state.upload_measurements.clear();
    conn.state.time_result_buffer.clear();
    conn.state.bytes_sent = 0;
    conn.state.read_pos = 0;
    conn.state.write_pos = 0;
    let (start, target) = if legacy {
        (TestPhase::PutSendCommand, TestPhase::PutCompleted)
    } else {
        (TestPhase::PerfSendCommand, TestPhase::PerfCompleted)
    };
    conn.state.phase = start;
    conn.set_interest(Interest::WRITABLE);
    // Same as the native `process_phase`: the phase clock starts at the command,
    // and the send handlers measure the upload duration from it.
    conn.state.phase_start_time = Some(Instant::now());
    pump_until(conn, ctx, target, duration_ms + PHASE_SLACK_MS, Track::Upload).await
}

/// `SIGNEDRESULT`: the server-signed envelope a control server accepts as proof
/// of the measurement.
async fn run_signed_result(conn: &mut Conn, ctx: &Ctx) -> Result<(), JsValue> {
    conn.state.read_pos = 0;
    conn.state.write_pos = 0;
    conn.state.phase = TestPhase::SignedResultSend;
    conn.set_interest(Interest::WRITABLE);
    conn.state.phase_start_time = Some(Instant::now());
    pump_until(
        conn,
        ctx,
        TestPhase::SignedResultCompleted,
        SIGNED_RESULT_TIMEOUT_MS,
        Track::None,
    )
    .await
}

/// Run one phase on every still-alive connection concurrently; a connection that
/// fails is logged and dropped from the rest of the measurement.
macro_rules! parallel_phase {
    ($conns:expr, $alive:expr, $ctx:expr, $label:expr, |$c:ident| $body:expr) => {{
        let snapshot: Vec<bool> = $alive.clone();
        let mut futures = Vec::new();
        for (i, $c) in $conns.iter_mut().enumerate() {
            if snapshot[i] {
                futures.push(async move { (i, $body.await) });
            }
        }
        for (i, result) in join_all(futures).await {
            if let Err(e) = result {
                $alive[i] = false;
                $ctx.log(&format!("{}: thread {} failed: {}", $label, i, err_str(&e)));
            }
        }
        $alive.iter().filter(|a| **a).count()
    }};
}

fn samples_of(conns: &[Conn], alive: &[bool], upload: bool) -> Vec<Vec<(u64, u64)>> {
    conns
        .iter()
        .filter(|c| alive[c.idx])
        .map(|c| {
            if upload {
                c.state.upload_measurements.iter().cloned().collect()
            } else {
                c.state.download_measurements.iter().cloned().collect()
            }
        })
        .filter(|v: &Vec<(u64, u64)>| !v.is_empty())
        .collect()
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Report the ping-derived jitter, used when the QUIC-datagram phase does not
/// run. Packet loss stays unreported in that case: TCP hides it.
fn log_ping_jitter_fallback(ctx: &Ctx, jitter_ms: Option<f64>) {
    match jitter_ms {
        Some(j) => ctx.log(&format!(
            "jitter: {j:.2} ms (ping RTT variation over TCP — approximate); packet loss not measurable"
        )),
        None => ctx.log("jitter: not enough ping samples"),
    }
}

/// Run the RMBT measurement from a browser.
///
/// ```js
/// const r = await run_measurement("wss://host:5443", {
///   threads: 3, downloadMs: 7000, uploadMs: 7000,
///   pretest: true, ping: true, download: true, upload: true,
///   legacyUpload: false, signedResult: false,
///   putTimeResultIntervalMs: 0, chunkSize: 0,
///   onProgress: (p) => console.log(p.phase, p.mbps),
/// }, (msg) => console.log(msg));
/// ```
///
/// Resolves to `{ pingMs, jitterMs, packetLossPercent, qosTransport,
/// downloadMbps, downloadBytes, uploadMbps, uploadBytes, chunkSize, threads,
/// durationMs, envelope }`. `jitterMs` is the RTT variation of the ping phase
/// and `packetLossPercent` come from the QUIC-datagram QoS phase when it can
/// run; `qosTransport` says which transport produced them (`"webtransport"`, or
/// `"websocket"` for the ping-RTT fallback, where packet loss stays null).
#[wasm_bindgen]
pub async fn run_measurement(
    url: String,
    options: JsValue,
    log_fn: Option<js_sys::Function>,
) -> Result<JsValue, JsValue> {
    // Back-compat with the two-argument PoC call `run_measurement(url, log)`.
    let (options, log_fn) = match options.dyn_ref::<js_sys::Function>() {
        Some(f) if log_fn.is_none() => (JsValue::UNDEFINED, Some(f.clone())),
        _ => (options, log_fn),
    };

    let threads = opt_u64(&options, "threads", DEFAULT_THREADS as u64).max(1) as usize;
    let download_ms = opt_u64(&options, "downloadMs", DEFAULT_DOWNLOAD_MS);
    let upload_ms = opt_u64(&options, "uploadMs", DEFAULT_UPLOAD_MS);
    let interim_ms = opt_u64(&options, "putTimeResultIntervalMs", 0);
    let forced_chunk = opt_u64(&options, "chunkSize", 0) as usize;
    let do_pretest = opt_bool(&options, "pretest", true);
    let do_ping = opt_bool(&options, "ping", true);
    let do_download = opt_bool(&options, "download", true);
    let do_upload = opt_bool(&options, "upload", true);
    let legacy_upload = opt_bool(&options, "legacyUpload", false);
    let do_signed_result = opt_bool(&options, "signedResult", false);
    let do_qos = opt_bool(&options, "qos", true);
    // Submitting the result is opt-in: a page pointed at a measurement server
    // has no business posting to a control server unless it was told to.
    let control_server = get_prop(&options, "controlServer").and_then(|v| v.as_string());
    let do_save = opt_bool(&options, "save", false) && control_server.is_some();
    let qos_packets = opt_u64(&options, "qosPackets", 10).max(2) as u32;
    let qos_delay_ms = opt_u64(&options, "qosDelayMs", 200).max(1);
    let progress_fn = opt_fn(&options, "onProgress");

    let ctx = Ctx::new(threads, log_fn, progress_fn);
    // Reborrow once: the per-connection futures below capture this `&Ctx` by
    // copy (`async move`), which a plain `&ctx` inside the loop could not do.
    let ctx = &ctx;
    let started = Instant::now();

    // ---- CONNECT ----
    ctx.begin_phase("connect");
    let mut conns: Vec<Conn> = Vec::with_capacity(threads);
    let connected = join_all((0..threads).map(|i| Conn::connect(&url, i))).await;
    for c in connected {
        match c {
            Ok(conn) => conns.push(conn),
            Err(e) => ctx.log(&format!("connect failed: {}", err_str(&e))),
        }
    }
    if conns.is_empty() {
        return Err(js_err(format!("could not connect to {url}")));
    }
    // Re-index so `idx` stays a valid slot in the progress vector.
    for (i, c) in conns.iter_mut().enumerate() {
        c.idx = i;
        c.state.token = Token(i);
    }
    let mut alive = vec![true; conns.len()];
    ctx.log(&format!(
        "connected {} ({} WebSocket{}, Rust/wasm handlers)",
        url,
        conns.len(),
        if conns.len() == 1 { "" } else { "s" }
    ));

    // ---- GREETING ----
    ctx.begin_phase("greeting");
    let ok = parallel_phase!(conns, alive, ctx, "greeting", |c| run_greeting(c, ctx));
    if ok == 0 {
        return Err(js_err("greeting failed on every connection"));
    }
    ctx.log("greeting: completed");

    // ---- PRETEST (GETCHUNKS) ----
    let mut chunk_size = if forced_chunk > 0 {
        snap_chunk_size(forced_chunk)
    } else {
        MIN_CHUNK_SIZE as usize
    };
    if do_pretest && forced_chunk == 0 {
        ctx.begin_phase("init");
        let ok = parallel_phase!(conns, alive, ctx, "pretest", |c| run_pretest(c, ctx));
        if ok == 0 {
            return Err(js_err("pre-test failed on every connection"));
        }
        chunk_size = conns
            .iter()
            .filter(|c| alive[c.idx])
            .map(|c| c.state.chunk_size)
            .max()
            .unwrap_or(MIN_CHUNK_SIZE as usize);
        ctx.log(&format!("pretest: chunk size {} KiB", chunk_size / 1024));
    }

    // ---- PING (single connection, like the native runner) ----
    let mut ping_ms = f64::NAN;
    let mut jitter_ms: Option<f64> = None;
    let mut packet_loss: Option<f64> = None;
    let mut qos_transport: Option<&str> = None;
    let mut qos_detail: Option<qos::QosResult> = None;
    if do_ping {
        ctx.begin_phase("ping");
        let idx = match alive.iter().position(|a| *a) {
            Some(i) => i,
            None => return Err(js_err("no connection left for ping")),
        };
        match run_ping(&mut conns[idx], ctx).await {
            Ok(()) => {
                ping_ms = conns[idx]
                    .state
                    .ping_median
                    .map(|ns| ns as f64 / 1e6)
                    .unwrap_or(f64::NAN);
                // Kept as the fallback jitter in case the QoS phase below
                // cannot run; logged only if it ends up being what we report,
                // so the log never shows two different jitter figures.
                jitter_ms = rfc3550_jitter_ns(&conns[idx].state.ping_times).map(|ns| ns / 1e6);
                if jitter_ms.is_some() {
                    qos_transport = Some("websocket");
                }
                ctx.log(&format!("ping: {ping_ms:.2} ms"));
            }
            Err(e) => {
                alive[idx] = false;
                ctx.log(&format!("ping failed: {}", err_str(&e)));
            }
        }
    }

    // ---- DOWNLOAD (GETTIME) ----
    let mut download_mbps = 0.0;
    let mut download_bytes = 0u64;
    if do_download {
        ctx.begin_phase("download");
        ctx.log(&format!(
            "download: {} thread(s) × {} ms, chunk {} KiB…",
            alive.iter().filter(|a| **a).count(),
            download_ms,
            chunk_size / 1024
        ));
        let ok = parallel_phase!(conns, alive, ctx, "download", |c| run_download(
            c, ctx, chunk_size, download_ms
        ));
        if ok == 0 {
            return Err(js_err("download failed on every connection"));
        }
        let per_thread = samples_of(&conns, &alive, false);
        download_mbps = calculate_download_speed_from_stats_silent(&per_thread).2;
        download_bytes = conns
            .iter()
            .filter(|c| alive[c.idx])
            .map(|c| c.state.bytes_received)
            .sum();
        ctx.log(&format!(
            "download: {download_mbps:.2} Mbit/s ({ok} thread(s), {:.1} MB)",
            download_bytes as f64 / 1e6
        ));
    }

    // ---- UPLOAD (PUTTIMERESULT / PUT) ----
    let mut upload_mbps = 0.0;
    let mut upload_bytes = 0u64;
    if do_upload {
        ctx.begin_phase("upload");
        ctx.log(&format!(
            "upload: {} thread(s) × {} ms, {}…",
            alive.iter().filter(|a| **a).count(),
            upload_ms,
            if legacy_upload { "PUT" } else { "PUTTIMERESULT" }
        ));
        let ok = parallel_phase!(conns, alive, ctx, "upload", |c| run_upload(
            c,
            ctx,
            chunk_size,
            upload_ms,
            legacy_upload,
            interim_ms
        ));
        if ok == 0 {
            return Err(js_err("upload failed on every connection"));
        }
        let per_thread = samples_of(&conns, &alive, true);
        upload_mbps = calculate_upload_speed_from_stats_silent(&per_thread).2;
        upload_bytes = conns
            .iter()
            .filter(|c| alive[c.idx])
            .map(|c| {
                c.state
                    .upload_measurements
                    .back()
                    .map(|(_, b)| *b)
                    .unwrap_or(c.state.bytes_sent)
            })
            .sum();
        ctx.log(&format!(
            "upload: {upload_mbps:.2} Mbit/s ({ok} thread(s), {:.1} MB)",
            upload_bytes as f64 / 1e6
        ));
    }

    // ---- QOS: jitter + packet loss over QUIC datagrams ----
    //
    // After the transfers, so the user sees throughput first. That costs some
    // accuracy the native order (before download) avoids: a link that has just
    // been saturated still has full queues, which inflates both jitter and
    // loss, so give it a moment to drain first. Never run it *during* a
    // transfer — that would measure our own event loop rather than the link.
    if do_qos {
        ctx.begin_phase("qos");
        sleep_ms(QOS_SETTLE_MS).await;
        if let Some(idx) = alive.iter().position(|a| *a) {
            match qos::run(&mut conns[idx], ctx, &url, qos_packets, qos_delay_ms).await {
                Ok(result) => {
                    jitter_ms = Some(result.jitter_ms());
                    packet_loss = Some(result.packet_loss_percent());
                    qos_transport = Some("webtransport");
                    qos_detail = Some(result);
                    ctx.log(&format!(
                        "qos: jitter {:.2} ms, packet loss {:.1}% (out {}/{} @ {:.2} ms, in {}/{} @ {:.2} ms)",
                        result.jitter_ms(),
                        result.packet_loss_percent(),
                        result.out.received,
                        result.out.sent,
                        result.out.jitter_ms,
                        result.inbound.received,
                        result.inbound.sent,
                        result.inbound.jitter_ms,
                    ));
                }
                // Not measurable here (no WebTransport, endpoint off, UDP
                // blocked): keep the ping-derived jitter and say so, rather
                // than failing a measurement that is otherwise fine.
                Err(e) => {
                    ctx.log(&format!("qos: unavailable ({})", err_str(&e)));
                    log_ping_jitter_fallback(ctx, jitter_ms);
                }
            }
        }
    }

    if !do_qos {
        log_ping_jitter_fallback(ctx, jitter_ms);
    }

    // ---- SAVE (control server) ----
    let mut open_test_uuid: Option<String> = None;
    if do_save {
        ctx.begin_phase("save");
        let request = save::SaveRequest {
            control_server: control_server.clone().unwrap_or_default(),
            client_uuid: save::client_uuid(),
            open_test_uuid: save::new_open_test_uuid(),
            measurement_server: qos::host_of(&url).to_string(),
            client_version: env!("CARGO_PKG_VERSION").to_string(),
            ping_ms,
            ping_times_ns: conns
                .iter()
                .find(|c| alive[c.idx])
                .map(|c| c.state.ping_times.clone())
                .unwrap_or_default(),
            jitter_ms,
            packet_loss_percent: packet_loss,
            download_mbps,
            upload_mbps,
            threads: alive.iter().filter(|a| **a).count(),
            samples: conns
                .iter()
                .filter(|c| alive[c.idx])
                .map(|c| save::ThreadSamples {
                    thread: c.idx,
                    download: c.state.download_measurements.iter().cloned().collect(),
                    upload: c.state.upload_measurements.iter().cloned().collect(),
                })
                .collect(),
        };
        match save::submit(&request).await {
            Ok(uuid) => {
                open_test_uuid = Some(uuid.clone());
                ctx.log(&format!("saved: {uuid}"));
            }
            // A measurement that could not be filed is still a measurement:
            // report the reason and hand the numbers back regardless.
            Err(e) => ctx.log(&format!("save failed: {}", save::describe_failure(&e))),
        }
    }

    // ---- SIGNEDRESULT (single connection) ----
    let mut envelope: Option<String> = None;
    if do_signed_result {
        ctx.begin_phase("signedresult");
        if let Some(idx) = alive.iter().position(|a| *a) {
            match run_signed_result(&mut conns[idx], ctx).await {
                Ok(()) => {
                    envelope = conns[idx].state.envelope.clone();
                    ctx.log("signed result: received");
                }
                Err(e) => ctx.log(&format!("signed result failed: {}", err_str(&e))),
            }
        }
    }

    for c in conns.iter_mut() {
        c.close();
    }
    ctx.begin_phase("done");

    let obj = js_sys::Object::new();
    let set = |k: &str, v: JsValue| {
        let _ = js_sys::Reflect::set(&obj, &JsValue::from_str(k), &v);
    };
    set(
        "openTestUuid",
        match &open_test_uuid {
            Some(uuid) => JsValue::from_str(uuid),
            None => JsValue::NULL,
        },
    );
    set("pingMs", JsValue::from_f64(ping_ms));
    set(
        "jitterMs",
        match jitter_ms {
            Some(j) => JsValue::from_f64(j),
            None => JsValue::NULL,
        },
    );
    // Null unless the QUIC-datagram phase ran: over TCP a browser cannot see
    // packet loss at all, and a retransmission-derived number would describe
    // the transport rather than the network.
    set(
        "packetLossPercent",
        match packet_loss {
            Some(loss) => JsValue::from_f64(loss),
            None => JsValue::NULL,
        },
    );
    set(
        "qosTransport",
        match qos_transport {
            Some(name) => JsValue::from_str(name),
            None => JsValue::NULL,
        },
    );
    // Per-direction detail: a one-sided loss (say upstream only) is a different
    // diagnosis from a symmetric one, and the combined figure hides it.
    set(
        "qos",
        match qos_detail {
            Some(result) => {
                let direction = |d: &qos::Direction| {
                    let obj = js_sys::Object::new();
                    let set = |k: &str, v: JsValue| {
                        let _ = js_sys::Reflect::set(&obj, &JsValue::from_str(k), &v);
                    };
                    set("sent", JsValue::from_f64(d.sent as f64));
                    set("received", JsValue::from_f64(d.received as f64));
                    set("lossPercent", JsValue::from_f64(d.loss_percent()));
                    set("jitterMs", JsValue::from_f64(d.jitter_ms));
                    JsValue::from(obj)
                };
                let obj = js_sys::Object::new();
                let _ = js_sys::Reflect::set(&obj, &"out".into(), &direction(&result.out));
                let _ = js_sys::Reflect::set(&obj, &"in".into(), &direction(&result.inbound));
                JsValue::from(obj)
            }
            None => JsValue::NULL,
        },
    );
    set("downloadMbps", JsValue::from_f64(download_mbps));
    set("downloadBytes", JsValue::from_f64(download_bytes as f64));
    set("uploadMbps", JsValue::from_f64(upload_mbps));
    set("uploadBytes", JsValue::from_f64(upload_bytes as f64));
    set("chunkSize", JsValue::from_f64(chunk_size as f64));
    set("threads", JsValue::from_f64(alive.iter().filter(|a| **a).count() as f64));
    set(
        "durationMs",
        JsValue::from_f64(started.elapsed().as_secs_f64() * 1000.0),
    );
    set(
        "envelope",
        match &envelope {
            Some(e) => JsValue::from_str(e.trim()),
            None => JsValue::NULL,
        },
    );
    Ok(obj.into())
}
