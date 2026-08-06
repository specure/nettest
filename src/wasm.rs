//! Browser WASM transport: a `WebSocket`-backed byte stream (`JsStream`) that
//! stands in for the native `mio::net::TcpStream`, plus a [`Reactor`] impl that
//! records interest for a JS-driven pump.
//!
//! This is the transport half of running the RMBT client in a browser. The
//! protocol/state-machine half (the handlers in `client::*`) is not wired up
//! here yet — that requires routing those handlers through [`crate::reactor`]
//! instead of mio (see the module docs). What compiles here proves the wasm
//! toolchain, the WebSocket transport and the reactor interface are sound.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::rc::Rc;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{BinaryType, MessageEvent, WebSocket};

use crate::reactor::{Reactor, Readiness};

/// Bytes buffered by the WebSocket `onmessage` handler, drained by `read()`.
type Inbox = Rc<RefCell<VecDeque<u8>>>;

/// A browser-`WebSocket`-backed byte stream substituting for the native
/// `mio::net::TcpStream`.
///
/// - `read()` drains bytes buffered from `onmessage`; an empty inbox yields
///   `WouldBlock`, exactly like a non-blocking TCP socket (which is what the
///   handlers already expect).
/// - `write()` forwards to `WebSocket.send`. It never blocks, so real
///   backpressure must be applied by the JS driver via [`JsStream::buffered_amount`]
///   before pushing more upload chunks.
pub struct JsStream {
    ws: WebSocket,
    inbox: Inbox,
    interest: Rc<RefCell<Readiness>>,
    // Kept alive for the lifetime of the stream so the JS callback isn't dropped.
    _onmessage: Closure<dyn FnMut(MessageEvent)>,
}

impl JsStream {
    /// Open a WebSocket to `url` (e.g. `wss://server/rmbt`) and start buffering
    /// incoming binary frames.
    pub fn connect(url: &str) -> Result<JsStream, JsValue> {
        let ws = WebSocket::new(url)?;
        ws.set_binary_type(BinaryType::Arraybuffer);

        let inbox: Inbox = Rc::new(RefCell::new(VecDeque::new()));
        let inbox_cb = inbox.clone();
        let onmessage = Closure::wrap(Box::new(move |e: MessageEvent| {
            if let Ok(buf) = e.data().dyn_into::<js_sys::ArrayBuffer>() {
                let arr = js_sys::Uint8Array::new(&buf);
                let mut bytes = vec![0u8; arr.length() as usize];
                arr.copy_to(&mut bytes);
                inbox_cb.borrow_mut().extend(bytes);
            }
        }) as Box<dyn FnMut(MessageEvent)>);
        ws.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));

        Ok(JsStream {
            ws,
            inbox,
            interest: Rc::new(RefCell::new(Readiness::default())),
            _onmessage: onmessage,
        })
    }

    /// A [`Reactor`] sharing this stream's interest cell, for the JS pump.
    pub fn reactor(&self) -> WasmReactor {
        WasmReactor { interest: self.interest.clone() }
    }

    /// Bytes still queued in the WebSocket send buffer — the browser's
    /// backpressure signal. The JS upload driver should stop feeding chunks
    /// while this is above a threshold.
    pub fn buffered_amount(&self) -> u32 {
        self.ws.buffered_amount()
    }

    /// Whether there is buffered incoming data to read.
    pub fn has_incoming(&self) -> bool {
        !self.inbox.borrow().is_empty()
    }
}

impl Read for JsStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut inbox = self.inbox.borrow_mut();
        if inbox.is_empty() {
            return Err(io::ErrorKind::WouldBlock.into());
        }
        let n = buf.len().min(inbox.len());
        for slot in buf.iter_mut().take(n) {
            *slot = inbox.pop_front().unwrap();
        }
        Ok(n)
    }
}

impl Write for JsStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.ws
            .send_with_u8_array(buf)
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "WebSocket.send failed"))?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Wasm [`Reactor`]: no OS poll — it just records which readiness the stream
/// wants, so the JS driver knows whether to pump reads (on `onmessage`) or
/// writes (on a send tick).
pub struct WasmReactor {
    interest: Rc<RefCell<Readiness>>,
}

impl WasmReactor {
    pub fn interest(&self) -> Readiness {
        *self.interest.borrow()
    }
}

impl Reactor for WasmReactor {
    fn set_interest(&self, _token: usize, interest: Readiness) {
        *self.interest.borrow_mut() = interest;
    }
}

/// Smoke-test export so the module is reachable from JS and the wasm-bindgen
/// surface is exercised at build time.
#[wasm_bindgen]
pub fn nettest_wasm_probe() -> String {
    "nettest wasm transport ready".to_string()
}
