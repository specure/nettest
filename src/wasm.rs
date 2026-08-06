//! Browser WASM RMBT client — the "real wasm path".
//!
//! The RMBT greeting → ping → download phases run **in Rust (wasm)** here; JS
//! only owns the `WebSocket`. A browser has no blocking `poll()`, so instead of
//! mio's event loop the state machine is expressed as `async`/`await`: reads are
//! futures that suspend when the inbox is empty and are woken by the WebSocket
//! `onmessage` callback. That is the wasm-native equivalent of the reactor/pump
//! model — the protocol logic drives itself; JS just feeds bytes.
//!
//! Jitter/packet-loss are UDP-only and intentionally omitted.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::task::{Poll, Waker};

use futures::future::poll_fn;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{BinaryType, MessageEvent, WebSocket};

const NL: u8 = b'\n';
const TERMINATOR: u8 = 0xff; // last byte of the last download chunk

#[derive(PartialEq)]
enum Mode {
    Line,
    Download,
}

struct DlState {
    bytes: u64,
    chunk_pos: usize,
    chunk_size: usize,
    done: bool,
}

/// Shared, single-threaded state between the async client and the JS callbacks.
struct Shared {
    buf: VecDeque<u8>,
    mode: Mode,
    dl: DlState,
    opened: bool,
    closed: bool,
    err: Option<String>,
    waker: Option<Waker>,
}

impl Shared {
    fn wake(&mut self) {
        if let Some(w) = self.waker.take() {
            w.wake();
        }
    }
}

/// A browser-`WebSocket`-backed RMBT byte stream driven by async reads.
struct JsStream {
    ws: WebSocket,
    shared: Rc<RefCell<Shared>>,
    // Kept alive for the socket's lifetime.
    _cbs: Vec<Closure<dyn FnMut(JsValue)>>,
    _onmsg: Closure<dyn FnMut(MessageEvent)>,
}

impl JsStream {
    fn connect(url: &str) -> Result<JsStream, JsValue> {
        let ws = WebSocket::new(url)?;
        ws.set_binary_type(BinaryType::Arraybuffer);

        let shared = Rc::new(RefCell::new(Shared {
            buf: VecDeque::new(),
            mode: Mode::Line,
            dl: DlState { bytes: 0, chunk_pos: 0, chunk_size: 4096, done: false },
            opened: false,
            closed: false,
            err: None,
            waker: None,
        }));

        // onmessage: feed line buffer or download counter, then wake the reader.
        let sh = shared.clone();
        let onmsg = Closure::wrap(Box::new(move |e: MessageEvent| {
            let bytes: Vec<u8> = if let Ok(ab) = e.data().dyn_into::<js_sys::ArrayBuffer>() {
                let arr = js_sys::Uint8Array::new(&ab);
                let mut v = vec![0u8; arr.length() as usize];
                arr.copy_to(&mut v);
                v
            } else if let Some(s) = e.data().as_string() {
                s.into_bytes()
            } else {
                return;
            };
            let mut st = sh.borrow_mut();
            if st.mode == Mode::Download {
                feed_download(&mut st, &bytes);
            } else {
                st.buf.extend(bytes);
            }
            st.wake();
        }) as Box<dyn FnMut(MessageEvent)>);
        ws.set_onmessage(Some(onmsg.as_ref().unchecked_ref()));

        let mut cbs: Vec<Closure<dyn FnMut(JsValue)>> = Vec::new();
        let sh_open = shared.clone();
        let onopen = Closure::wrap(Box::new(move |_e: JsValue| {
            let mut st = sh_open.borrow_mut();
            st.opened = true;
            st.wake();
        }) as Box<dyn FnMut(JsValue)>);
        ws.set_onopen(Some(onopen.as_ref().unchecked_ref()));
        cbs.push(onopen);

        let sh_close = shared.clone();
        let onclose = Closure::wrap(Box::new(move |_e: JsValue| {
            let mut st = sh_close.borrow_mut();
            st.closed = true;
            st.wake();
        }) as Box<dyn FnMut(JsValue)>);
        ws.set_onclose(Some(onclose.as_ref().unchecked_ref()));
        cbs.push(onclose);

        let sh_err = shared.clone();
        let onerror = Closure::wrap(Box::new(move |_e: JsValue| {
            let mut st = sh_err.borrow_mut();
            if st.err.is_none() {
                st.err = Some("websocket error".to_string());
            }
            st.wake();
        }) as Box<dyn FnMut(JsValue)>);
        ws.set_onerror(Some(onerror.as_ref().unchecked_ref()));
        cbs.push(onerror);

        Ok(JsStream { ws, shared, _cbs: cbs, _onmsg: onmsg })
    }

    fn send(&self, bytes: &[u8]) -> Result<(), JsValue> {
        self.ws.send_with_u8_array(bytes)
    }

    fn buffered_amount(&self) -> u32 {
        self.ws.buffered_amount()
    }

    async fn open(&self) -> Result<(), JsValue> {
        let sh = self.shared.clone();
        poll_fn(move |cx| {
            let mut st = sh.borrow_mut();
            if let Some(e) = &st.err {
                return Poll::Ready(Err(JsValue::from_str(e)));
            }
            if st.opened {
                return Poll::Ready(Ok(()));
            }
            st.waker = Some(cx.waker().clone());
            Poll::Pending
        })
        .await
    }

    async fn read_line(&self) -> Result<String, JsValue> {
        let sh = self.shared.clone();
        poll_fn(move |cx| {
            let mut st = sh.borrow_mut();
            if let Some(pos) = st.buf.iter().position(|&b| b == NL) {
                let line: Vec<u8> = st.buf.drain(..=pos).take(pos).collect();
                let s = String::from_utf8_lossy(&line).trim_end_matches('\r').to_string();
                return Poll::Ready(Ok(s));
            }
            if let Some(e) = &st.err {
                return Poll::Ready(Err(JsValue::from_str(e)));
            }
            if st.closed {
                return Poll::Ready(Err(JsValue::from_str("closed")));
            }
            st.waker = Some(cx.waker().clone());
            Poll::Pending
        })
        .await
    }

    async fn read_until(&self, prefix: &str) -> Result<String, JsValue> {
        loop {
            let line = self.read_line().await?;
            if line.starts_with(prefix) {
                return Ok(line);
            }
        }
    }

    /// Count bytes (discarding data) until a chunk ends with 0xFF. Returns total.
    async fn download(&self, chunk_size: usize) -> Result<u64, JsValue> {
        {
            let mut st = self.shared.borrow_mut();
            st.dl = DlState { bytes: 0, chunk_pos: 0, chunk_size, done: false };
            st.mode = Mode::Download;
            // Feed any bytes already buffered from line mode.
            let leftover: Vec<u8> = st.buf.drain(..).collect();
            if !leftover.is_empty() {
                feed_download(&mut st, &leftover);
            }
        }
        let sh = self.shared.clone();
        poll_fn(move |cx| {
            let mut st = sh.borrow_mut();
            if st.dl.done {
                st.mode = Mode::Line;
                return Poll::Ready(Ok(st.dl.bytes));
            }
            if let Some(e) = &st.err {
                return Poll::Ready(Err(JsValue::from_str(e)));
            }
            if st.closed {
                return Poll::Ready(Err(JsValue::from_str("closed during download")));
            }
            st.waker = Some(cx.waker().clone());
            Poll::Pending
        })
        .await
    }
}

/// Count bytes in `bytes` toward the current chunk; on a chunk ending 0xFF mark
/// download done and push any trailing bytes back into the line buffer.
fn feed_download(st: &mut Shared, bytes: &[u8]) {
    let chunk_size = st.dl.chunk_size;
    let mut i = 0;
    while i < bytes.len() {
        let remaining = chunk_size - st.dl.chunk_pos;
        let take = remaining.min(bytes.len() - i);
        st.dl.chunk_pos += take;
        st.dl.bytes += take as u64;
        i += take;
        if st.dl.chunk_pos == chunk_size {
            let flag = bytes[i - 1];
            st.dl.chunk_pos = 0;
            if flag == TERMINATOR {
                st.dl.done = true;
                if i < bytes.len() {
                    st.buf.extend(&bytes[i..]);
                }
                return;
            }
        }
    }
}

fn now_ms() -> f64 {
    web_sys::window()
        .and_then(|w| w.performance())
        .map(|p| p.now())
        .unwrap_or(0.0)
}

fn log(f: &js_sys::Function, msg: &str) {
    let _ = f.call1(&JsValue::NULL, &JsValue::from_str(msg));
}

/// Run greeting → ping → download entirely in Rust/wasm over a browser
/// WebSocket. `log_fn(msg: string)` receives progress lines. Resolves to
/// `{ pingMs, downloadMbps, downloadBytes }`.
#[wasm_bindgen]
pub async fn run_measurement(url: String, log_fn: js_sys::Function) -> Result<JsValue, JsValue> {
    let io = JsStream::connect(&url)?;
    io.open().await?;
    log(&log_fn, &format!("connected {url}"));

    // ---- GREETING ----
    let version = io.read_line().await?;
    log(&log_fn, &format!("greeting: {version}"));
    io.read_until("ACCEPT TOKEN").await?;
    io.send(format!("TOKEN {}_wasm\n", uuid::Uuid::new_v4()).as_bytes())?;

    let mut chunk_size: usize = 4096;
    loop {
        let l = io.read_line().await?;
        if let Some(rest) = l.strip_prefix("CHUNKSIZE ") {
            if let Some(cs) = rest.split_whitespace().next().and_then(|s| s.parse().ok()) {
                chunk_size = cs;
            }
        }
        if l.starts_with("ACCEPT") {
            break;
        }
    }
    log(&log_fn, &format!("token accepted, chunkSize={chunk_size}"));

    // ---- PING (5 samples; RTT = client PING->PONG) ----
    let mut best = f64::INFINITY;
    for i in 0..5 {
        if i > 0 {
            io.read_until("ACCEPT").await?;
        }
        let t0 = now_ms();
        io.send(b"PING\n")?;
        io.read_until("PONG").await?;
        let rtt = now_ms() - t0;
        io.send(b"OK\n")?;
        io.read_until("TIME").await?;
        if rtt < best {
            best = rtt;
        }
    }
    log(&log_fn, &format!("ping: {best:.2} ms"));

    // ---- DOWNLOAD (GETTIME) ----
    io.read_until("ACCEPT").await?;
    let duration_sec = 2u32;
    io.send(format!("GETTIME {duration_sec} {chunk_size}\n").as_bytes())?;
    let dl_start = now_ms();
    let bytes = io.download(chunk_size).await?;
    let _ = io.buffered_amount(); // (backpressure hook — unused for download)
    io.send(b"OK\n")?;
    let time_line = io.read_until("TIME").await?;
    let server_ns: f64 = time_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0);
    let secs = if server_ns > 0.0 { server_ns / 1e9 } else { (now_ms() - dl_start) / 1000.0 };
    let mbps = (bytes as f64 * 8.0) / secs / 1e6;
    log(&log_fn, &format!("download: {mbps:.2} Mbit/s ({:.1} MB in {secs:.2} s)", bytes as f64 / 1e6));

    let _ = io.ws.close();

    let obj = js_sys::Object::new();
    js_sys::Reflect::set(&obj, &"pingMs".into(), &best.into())?;
    js_sys::Reflect::set(&obj, &"downloadMbps".into(), &mbps.into())?;
    js_sys::Reflect::set(&obj, &"downloadBytes".into(), &(bytes as f64).into())?;
    Ok(obj.into())
}
