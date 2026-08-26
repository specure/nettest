//! Submitting a finished browser measurement to the control server.
//!
//! Posts to `{control_server}/browser/save`, the registration-less endpoint:
//! the client makes up its own `open_test_uuid`, so there is nothing to
//! register before a test and nothing to reconcile after one.
//!
//! Only what the client legitimately knows is sent, and only what is actually
//! stored: the speeds go as the client computed them (RMBT — first second
//! skipped, interpolated at t*), with no byte totals or phase durations, which
//! reach neither Elasticsearch nor any table and would only invite the server to
//! recompute a different number.
//!
//! Beyond that, Provider, ASN, geo and the
//! measurement server's identity are resolved by the control server from the
//! address the request arrives on — a browser cannot be trusted to state them,
//! and the server would overwrite them anyway.

use serde_json::{json, Map, Value};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

use crate::client::graph::speed_curve;
use crate::client::user_agent::parse_user_agent;
use crate::wasm::{err_str, js_err};

/// One connection's `(time_ns, cumulative_bytes)` samples for one direction.
pub struct ThreadSamples {
    pub thread: usize,
    pub download: Vec<(u64, u64)>,
    pub upload: Vec<(u64, u64)>,
}

/// Everything the save endpoint takes that only the client can know.
pub struct SaveRequest {
    pub control_server: String,
    pub client_uuid: String,
    pub open_test_uuid: String,
    /// Host the measurement ran against, as the page addressed it.
    pub measurement_server: String,
    pub client_version: String,
    pub ping_ms: f64,
    pub ping_times_ns: Vec<u64>,
    pub jitter_ms: Option<f64>,
    pub packet_loss_percent: Option<f64>,
    pub download_mbps: f64,
    pub upload_mbps: f64,
    pub threads: usize,
    pub samples: Vec<ThreadSamples>,
}

impl SaveRequest {
    fn download_samples(&self) -> Vec<Vec<(u64, u64)>> {
        self.samples.iter().map(|t| t.download.clone()).collect()
    }

    fn upload_samples(&self) -> Vec<Vec<(u64, u64)>> {
        self.samples.iter().map(|t| t.upload.clone()).collect()
    }
}

/// POST the result. Resolves to the `open_test_uuid` it was stored under.
pub async fn submit(request: &SaveRequest) -> Result<String, JsValue> {
    let url = format!("{}/browser/save", request.control_server.trim_end_matches('/'));
    let body = serde_json::to_string(&payload(request))
        .map_err(|e| js_err(format!("cannot serialise the result: {e}")))?;

    let global = js_sys::global();
    let fetch: js_sys::Function = js_sys::Reflect::get(&global, &JsValue::from_str("fetch"))?
        .dyn_into()
        .map_err(|_| js_err("this environment has no fetch"))?;

    let headers = js_sys::Object::new();
    js_sys::Reflect::set(&headers, &"Content-Type".into(), &"application/json".into())?;
    // The control server routes tenants by this header.
    js_sys::Reflect::set(&headers, &"X-Nettest-Client".into(), &"nt".into())?;

    let init = js_sys::Object::new();
    js_sys::Reflect::set(&init, &"method".into(), &"POST".into())?;
    js_sys::Reflect::set(&init, &"headers".into(), &headers)?;
    js_sys::Reflect::set(&init, &"body".into(), &JsValue::from_str(&body))?;

    let response = JsFuture::from(js_sys::Promise::from(
        fetch.call2(&global, &JsValue::from_str(&url), &init)?,
    ))
    .await?;

    let ok = js_sys::Reflect::get(&response, &"ok".into())?.as_bool().unwrap_or(false);
    if !ok {
        let status = js_sys::Reflect::get(&response, &"status".into())?
            .as_f64()
            .unwrap_or(0.0);
        // The endpoint answers a rejected body with the offending field names;
        // surfacing them beats reporting a bare status.
        let detail = read_text(&response).await.unwrap_or_default();
        return Err(js_err(format!("{url} answered {status}: {detail}")));
    }

    Ok(request.open_test_uuid.clone())
}

async fn read_text(response: &JsValue) -> Option<String> {
    let text: js_sys::Function = js_sys::Reflect::get(response, &"text".into()).ok()?.dyn_into().ok()?;
    let promise = js_sys::Promise::from(text.call0(response).ok()?);
    JsFuture::from(promise).await.ok()?.as_string()
}

/// Build the request body the browser save endpoint expects.
fn payload(request: &SaveRequest) -> Value {
    let (browser_name, browser_version, platform) = browser_identity();

    // The curve, not the raw per-thread samples. Interpolating the threads onto
    // one timeline is what the result page draws, and doing it here means the
    // stored graph is the one the user watched — the server's own aggregation
    // truncates every thread to the shortest one's sample count, which on a
    // browser measurement (where threads report at very different rates) would
    // throw away most of the transfer. Sent as a single series, the way the
    // Flutter client sends its curve.
    let mut speed_detail: Vec<Value> = Vec::new();
    push_curve(&mut speed_detail, "download", &request.download_samples());
    push_curve(&mut speed_detail, "upload", &request.upload_samples());

    let pings: Vec<Value> = request
        .ping_times_ns
        .iter()
        .map(|ns| json!({ "value": ns, "value_server": ns }))
        .collect();

    let mut body = Map::new();
    body.insert("open_test_uuid".into(), json!(request.open_test_uuid));
    body.insert("client_uuid".into(), json!(request.client_uuid));
    body.insert("measurement_server_ip".into(), json!(request.measurement_server));
    body.insert("client_name".into(), json!("RMBTws"));
    body.insert("client_version".into(), json!(request.client_version));
    body.insert("client_language".into(), json!(navigator_string("language")));
    body.insert("browser_name".into(), json!(browser_name));
    body.insert("browser_version".into(), json!(browser_version));
    body.insert("plattform".into(), json!(platform));
    body.insert("time".into(), json!(js_sys::Date::now() as u64));
    body.insert("test_status".into(), json!("0"));
    // 98 = LAN. A browser cannot see whether it is on Wi-Fi or Ethernet, and
    // guessing would put a wrong network type on every record.
    body.insert("network_type".into(), json!(98));
    body.insert("test_num_threads".into(), json!(request.threads));
    body.insert("test_speed_download".into(), json!((request.download_mbps * 1000.0).round() as i64));
    body.insert("test_speed_upload".into(), json!((request.upload_mbps * 1000.0).round() as i64));
    body.insert("test_ping_shortest".into(), json!(shortest_ping_ns(request)));
    body.insert("pings".into(), Value::Array(pings));
    body.insert("speed_detail".into(), Value::Array(speed_detail));
    if let Some(jitter) = request.jitter_ms {
        body.insert("voip_result_jitter_millis".into(), json!(format!("{jitter:.3}")));
    }
    if let Some(loss) = request.packet_loss_percent {
        body.insert("voip_result_packet_loss_percents".into(), json!(format!("{loss:.2}")));
    }
    Value::Object(body)
}

fn shortest_ping_ns(request: &SaveRequest) -> u64 {
    request
        .ping_times_ns
        .iter()
        .copied()
        .min()
        .unwrap_or_else(|| (request.ping_ms * 1e6) as u64)
}

/// Aggregate one direction's per-connection samples into a curve and append it.
fn push_curve(into: &mut Vec<Value>, direction: &str, per_thread: &[Vec<(u64, u64)>]) {
    for point in speed_curve(per_thread) {
        into.push(json!({
            "direction": direction,
            // One series: the threads have already been summed into it.
            "thread": 0,
            "time": point.time_elapsed_ms * 1_000_000,
            "bytes": point.bytes_total,
        }));
    }
}

/// Browser name, version and operating system.
///
/// From `navigator.userAgentData` where it exists (Chromium), otherwise a small
/// user-agent match. Only used for display in the history list, so an unknown
/// browser is left null rather than guessed at.
fn browser_identity() -> (Option<String>, Option<String>, Option<String>) {
    let navigator = match js_sys::Reflect::get(&js_sys::global(), &JsValue::from_str("navigator")) {
        Ok(navigator) if !navigator.is_undefined() => navigator,
        _ => return (None, None, None),
    };

    let platform = js_sys::Reflect::get(&navigator, &JsValue::from_str("userAgentData"))
        .ok()
        .filter(|data| !data.is_undefined())
        .and_then(|data| js_sys::Reflect::get(&data, &JsValue::from_str("platform")).ok())
        .and_then(|value| value.as_string())
        .or_else(|| navigator_string("platform"));

    let user_agent = navigator_string("userAgent").unwrap_or_default();
    let (name, version) = parse_user_agent(&user_agent);
    (name, version, platform)
}

fn navigator_string(property: &str) -> Option<String> {
    let navigator = js_sys::Reflect::get(&js_sys::global(), &JsValue::from_str("navigator")).ok()?;
    js_sys::Reflect::get(&navigator, &JsValue::from_str(property))
        .ok()?
        .as_string()
}

/// The client UUID identifies the browser across measurements, so history can
/// list them together. Stored in `localStorage`; where that is unavailable
/// (private mode, a worker, Node) each run simply looks like a new client.
pub fn client_uuid() -> String {
    const KEY: &str = "nettest_client_uuid";

    let storage = js_sys::Reflect::get(&js_sys::global(), &JsValue::from_str("localStorage"))
        .ok()
        .filter(|storage| !storage.is_undefined() && !storage.is_null());

    if let Some(storage) = &storage {
        if let Some(existing) = call_storage(storage, "getItem", &[JsValue::from_str(KEY)])
            .and_then(|value| value.as_string())
            .filter(|value| !value.is_empty())
        {
            return existing;
        }
    }

    let fresh = uuid::Uuid::new_v4().to_string();
    if let Some(storage) = &storage {
        let _ = call_storage(
            storage,
            "setItem",
            &[JsValue::from_str(KEY), JsValue::from_str(&fresh)],
        );
    }
    fresh
}

fn call_storage(storage: &JsValue, method: &str, args: &[JsValue]) -> Option<JsValue> {
    let function: js_sys::Function = js_sys::Reflect::get(storage, &JsValue::from_str(method))
        .ok()?
        .dyn_into()
        .ok()?;
    let argv = js_sys::Array::new();
    for arg in args {
        argv.push(arg);
    }
    js_sys::Reflect::apply(&function, storage, &argv).ok()
}

/// A fresh id for one measurement. The control server stores the result under
/// it, so it is also the link to the result page.
pub fn new_open_test_uuid() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Turn a submission failure into something a log line can carry.
pub fn describe_failure(error: &JsValue) -> String {
    err_str(error)
}
