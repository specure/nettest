//! Browser WASM driver — the "real handlers" path, multi-connection.
//!
//! Runs the RMBT greeting → ping → download phases **in Rust (wasm)** by driving
//! the SAME handler functions the native client uses (`client::handlers::*` via
//! `basic_handler`), over `Stream::Js(JsWss)` WebSockets. A browser has no
//! blocking `poll()`, so the async `pump` replaces the native mio event loop.
//!
//! Download runs over N WebSockets concurrently (the browser analogue of the
//! native thread pool) driven together on the single JS event loop via
//! `join_all`; per-connection samples are aggregated exactly like the native
//! client (`calculate_download_speed_from_stats_silent`).
//!
//! Jitter/packet-loss are UDP-only and omitted.

use std::task::Poll as TaskPoll;

use futures::future::{join_all, poll_fn};
use wasm_bindgen::prelude::*;
use web_time::Instant;

use crate::client::calculator::calculate_download_speed_from_stats_silent;
use crate::client::handlers::basic_handler::{
    handle_client_readable_data, handle_client_writable_data,
};
use crate::client::state::{MeasurementState, TestPhase};
use crate::reactor::{Interest, Poll, Token};
use crate::stream::js_wss::{JsWss, Notify};
use crate::stream::stream::Stream;

/// Parallel download connections (browser analogue of the native thread count).
const THREADS: usize = 3;
/// Download phase duration.
const DOWNLOAD_MS: u64 = 5000;
/// Chunk size requested in GETTIME (bytes). Larger chunks = far less per-chunk
/// overhead than the 4 KiB minimum.
const CHUNK_SIZE: usize = 256 * 1024;

fn log(f: &js_sys::Function, msg: &str) {
    let _ = f.call1(&JsValue::NULL, &JsValue::from_str(msg));
}

async fn await_open(n: &Notify) -> Result<(), JsValue> {
    poll_fn(|cx| {
        if n.is_open() {
            TaskPoll::Ready(Ok(()))
        } else if n.is_closed() {
            TaskPoll::Ready(Err(JsValue::from_str("socket closed before open")))
        } else {
            n.set_waker(cx.waker());
            TaskPoll::Pending
        }
    })
    .await
}

async fn await_readable(n: &Notify) -> Result<(), JsValue> {
    poll_fn(|cx| {
        if n.has_incoming() {
            TaskPoll::Ready(Ok(()))
        } else if n.is_closed() {
            TaskPoll::Ready(Err(JsValue::from_str("socket closed")))
        } else {
            n.set_waker(cx.waker());
            TaskPoll::Pending
        }
    })
    .await
}

fn interest(state: &MeasurementState) -> Interest {
    match &state.stream {
        Stream::Js(s) => s.interest(),
    }
}

fn set_writable(state: &mut MeasurementState, poll: &Poll) {
    let token = state.token;
    if let Stream::Js(s) = &mut state.stream {
        let _ = s.reregister(poll, token, Interest::WRITABLE);
    }
}

/// Async analogue of the native `TestState::process_phase`: drive the shared
/// handlers until `state.phase == target`.
async fn pump_until(
    state: &mut MeasurementState,
    notify: &Notify,
    poll: &Poll,
    target: TestPhase,
) -> Result<(), JsValue> {
    loop {
        if state.phase == target {
            return Ok(());
        }
        let r = if interest(state).is_writable() {
            handle_client_writable_data(state, poll)
        } else {
            if !notify.has_incoming() {
                await_readable(notify).await?;
                continue;
            }
            handle_client_readable_data(state, poll)
        };
        match r {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                await_readable(notify).await?;
            }
            Err(e) => return Err(JsValue::from_str(&format!("handler error: {e}"))),
        }
    }
}

fn dummy_addr() -> std::net::SocketAddr {
    std::net::SocketAddr::from(([0u8, 0, 0, 0], 0))
}

/// Connect + greeting on one WebSocket; returns the driven state and its notify.
async fn connect_and_greet(url: &str, tok: usize) -> Result<(MeasurementState, Notify, Poll), JsValue> {
    let poll = Poll::new().map_err(|e| JsValue::from_str(&e.to_string()))?;
    let wss = JsWss::connect(url)?;
    let notify = wss.notify();
    let mut state = MeasurementState::new(Stream::Js(wss), Token(tok), dummy_addr());
    await_open(&notify).await?;
    state.phase_start_time = Some(Instant::now());
    pump_until(&mut state, &notify, &poll, TestPhase::GreetingCompleted).await?;
    Ok((state, notify, poll))
}

/// One download connection: greeting → GETTIME → return its per-chunk samples.
async fn download_one(url: String, tok: usize) -> Result<(Vec<(u64, u64)>, u64), JsValue> {
    let (mut state, notify, poll) = connect_and_greet(&url, tok).await?;
    state.chunk_size = CHUNK_SIZE;
    state.download_duration_ms = DOWNLOAD_MS;
    state.phase = TestPhase::GetTimeSendCommand;
    set_writable(&mut state, &poll);
    state.phase_start_time = Some(Instant::now());
    pump_until(&mut state, &notify, &poll, TestPhase::GetTimeCompleted).await?;
    let samples = state.download_measurements.iter().cloned().collect();
    let bytes = state.bytes_received;
    if let Stream::Js(s) = &mut state.stream {
        let _ = s.close();
    }
    Ok((samples, bytes))
}

/// Run greeting → ping → (N-connection) download, all via the real Rust handlers
/// over browser WebSockets. Resolves to `{ pingMs, downloadMbps, threads,
/// downloadBytes }`.
#[wasm_bindgen]
pub async fn run_measurement(url: String, log_fn: js_sys::Function) -> Result<JsValue, JsValue> {
    // ---- GREETING + PING on one connection ----
    let (mut state, notify, poll) = connect_and_greet(&url, 1).await?;
    log(&log_fn, &format!("connected {url} (real Rust/wasm handlers, {THREADS} threads)"));
    log(&log_fn, "greeting: completed");
    state.phase = TestPhase::PingSendPing;
    set_writable(&mut state, &poll);
    state.phase_start_time = Some(Instant::now());
    pump_until(&mut state, &notify, &poll, TestPhase::PingCompleted).await?;
    let ping_ms = state.ping_median.map(|ns| ns as f64 / 1e6).unwrap_or(f64::NAN);
    log(&log_fn, &format!("ping: {ping_ms:.2} ms"));
    if let Stream::Js(s) = &mut state.stream {
        let _ = s.close();
    }
    drop(state);

    // ---- DOWNLOAD over N connections concurrently ----
    log(&log_fn, &format!("download: starting {THREADS}×{DOWNLOAD_MS}ms, chunk={}KiB…", CHUNK_SIZE / 1024));
    let tasks = (0..THREADS).map(|i| download_one(url.clone(), 100 + i));
    let results = join_all(tasks).await;

    let mut per_thread: Vec<Vec<(u64, u64)>> = Vec::new();
    let mut total_bytes: u64 = 0;
    let mut ok_threads = 0usize;
    for r in results {
        match r {
            Ok((samples, bytes)) => {
                total_bytes += bytes;
                if !samples.is_empty() {
                    per_thread.push(samples);
                }
                ok_threads += 1;
            }
            Err(e) => log(&log_fn, &format!("download thread failed: {e:?}")),
        }
    }

    let (bps, _gbps, _) = calculate_download_speed_from_stats_silent(&per_thread);
    let mbps = bps / 1e6;
    log(
        &log_fn,
        &format!(
            "download: {mbps:.2} Mbit/s ({ok_threads}/{THREADS} threads, {:.1} MB total)",
            total_bytes as f64 / 1e6
        ),
    );

    let obj = js_sys::Object::new();
    js_sys::Reflect::set(&obj, &"pingMs".into(), &ping_ms.into())?;
    js_sys::Reflect::set(&obj, &"downloadMbps".into(), &mbps.into())?;
    js_sys::Reflect::set(&obj, &"threads".into(), &(ok_threads as f64).into())?;
    js_sys::Reflect::set(&obj, &"downloadBytes".into(), &(total_bytes as f64).into())?;
    Ok(obj.into())
}
