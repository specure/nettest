//! Browser WebSocket transport for the `Stream` interface (wasm only).
//!
//! `JsWss` is the wasm sibling of the native `mio::net::TcpStream` /
//! `WebSocketClient` variants: a **non-blocking** byte stream backed by a
//! browser `WebSocket`. Reads drain bytes buffered by `onmessage` and return
//! `WouldBlock` when empty — exactly the contract the RMBT handlers already
//! expect from a non-blocking socket. Writes go straight to `WebSocket.send`.
//!
//! `register`/`reregister` don't touch any OS poll (there is none in a browser);
//! they just record the interest so the wasm pump (the async driver that
//! replaces `poll.poll()`) knows whether to feed reads or drive writes. A
//! [`Notify`] handle lets that pump `await` new data / socket-open without
//! holding a borrow on the stream.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::rc::Rc;
use std::task::Waker;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{BinaryType, MessageEvent, WebSocket};

use crate::reactor::{Interest, Poll, Token};

/// Browser send-buffer high-water mark: `write` reports `WouldBlock` above it,
/// so the caller yields to the JS event loop instead of queueing the whole
/// upload in the tab's memory.
///
/// Keep it small. It bounds the data queued in the tab but not yet on the wire,
/// which on a slow uplink is a tail the server still has to receive after the
/// send window closes (512 KiB is ~0.4 s at 10 Mbit/s). It does *not* bound
/// throughput: that depends on how often the driver can refill the buffer, and
/// `wasm::yield_now` is unclamped — measured upload was identical at 512 KiB,
/// 1 MiB and 4 MiB.
const SEND_HIGH_WATER: u32 = 512 * 1024;
/// Largest single `WebSocket.send`, so backpressure is re-checked often enough
/// even with a multi-megabyte RMBT chunk.
const MAX_SEND: usize = 256 * 1024;

type Inbox = Rc<RefCell<VecDeque<u8>>>;
type WakerCell = Rc<RefCell<Option<Waker>>>;

/// A pump-side handle to await readability / socket-open without borrowing the
/// stream (which the handlers hold `&mut`).
#[derive(Clone)]
pub struct Notify {
    inbox: Inbox,
    opened: Rc<Cell<bool>>,
    closed: Rc<Cell<bool>>,
    waker: WakerCell,
}

impl Notify {
    pub fn has_incoming(&self) -> bool {
        !self.inbox.borrow().is_empty()
    }
    pub fn is_open(&self) -> bool {
        self.opened.get()
    }
    pub fn is_closed(&self) -> bool {
        self.closed.get()
    }
    pub fn set_waker(&self, w: &Waker) {
        *self.waker.borrow_mut() = Some(w.clone());
    }
}

fn wake(cell: &WakerCell) {
    if let Some(w) = cell.borrow_mut().take() {
        w.wake();
    }
}

pub struct JsWss {
    ws: WebSocket,
    inbox: Inbox,
    interest: Rc<Cell<Interest>>,
    opened: Rc<Cell<bool>>,
    closed: Rc<Cell<bool>>,
    waker: WakerCell,
    _cbs: Vec<Closure<dyn FnMut(JsValue)>>,
    _onmessage: Closure<dyn FnMut(MessageEvent)>,
}

impl std::fmt::Debug for JsWss {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "JsWss(buffered={})", self.ws.buffered_amount())
    }
}

impl JsWss {
    pub fn connect(url: &str) -> Result<JsWss, JsValue> {
        let ws = WebSocket::new(url)?;
        ws.set_binary_type(BinaryType::Arraybuffer);

        let inbox: Inbox = Rc::new(RefCell::new(VecDeque::new()));
        let opened = Rc::new(Cell::new(false));
        let closed = Rc::new(Cell::new(false));
        let waker: WakerCell = Rc::new(RefCell::new(None));

        let inbox_cb = inbox.clone();
        let waker_msg = waker.clone();
        let onmessage = Closure::wrap(Box::new(move |e: MessageEvent| {
            if let Ok(ab) = e.data().dyn_into::<js_sys::ArrayBuffer>() {
                let arr = js_sys::Uint8Array::new(&ab);
                let mut v = vec![0u8; arr.length() as usize];
                arr.copy_to(&mut v);
                inbox_cb.borrow_mut().extend(v);
            } else if let Some(s) = e.data().as_string() {
                inbox_cb.borrow_mut().extend(s.into_bytes());
            }
            wake(&waker_msg);
        }) as Box<dyn FnMut(MessageEvent)>);
        ws.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));

        let mut cbs: Vec<Closure<dyn FnMut(JsValue)>> = Vec::new();

        let opened_cb = opened.clone();
        let waker_open = waker.clone();
        let onopen = Closure::wrap(Box::new(move |_e: JsValue| {
            opened_cb.set(true);
            wake(&waker_open);
        }) as Box<dyn FnMut(JsValue)>);
        ws.set_onopen(Some(onopen.as_ref().unchecked_ref()));
        cbs.push(onopen);

        let closed_cb = closed.clone();
        let waker_close = waker.clone();
        let onclose = Closure::wrap(Box::new(move |_e: JsValue| {
            closed_cb.set(true);
            wake(&waker_close);
        }) as Box<dyn FnMut(JsValue)>);
        ws.set_onclose(Some(onclose.as_ref().unchecked_ref()));
        cbs.push(onclose);

        Ok(JsWss {
            ws,
            inbox,
            // The RMBT flow starts by sending, so begin interested in writes.
            interest: Rc::new(Cell::new(Interest::WRITABLE)),
            opened,
            closed,
            waker,
            _cbs: cbs,
            _onmessage: onmessage,
        })
    }

    pub fn notify(&self) -> Notify {
        Notify {
            inbox: self.inbox.clone(),
            opened: self.opened.clone(),
            closed: self.closed.clone(),
            waker: self.waker.clone(),
        }
    }

    pub fn interest(&self) -> Interest {
        self.interest.get()
    }

    pub fn has_incoming(&self) -> bool {
        !self.inbox.borrow().is_empty()
    }

    /// The browser's send-buffer backpressure signal.
    pub fn buffered_amount(&self) -> u32 {
        self.ws.buffered_amount()
    }

    /// True when `write` would accept data (socket open and below the
    /// high-water mark). The pump uses it to wait out backpressure.
    pub fn is_writable(&self) -> bool {
        self.ws.ready_state() == WebSocket::OPEN && self.ws.buffered_amount() < SEND_HIGH_WATER
    }

    pub fn register(&mut self, _poll: &Poll, _token: Token, interest: Interest) -> io::Result<()> {
        self.interest.set(interest);
        Ok(())
    }

    pub fn reregister(&mut self, _poll: &Poll, _token: Token, interest: Interest) -> io::Result<()> {
        self.interest.set(interest);
        Ok(())
    }

    /// Detach all JS event handlers so a subsequent event (e.g. onclose) can't
    /// invoke a Rust closure that is about to be / has been dropped.
    fn detach(&self) {
        self.ws.set_onmessage(None);
        self.ws.set_onopen(None);
        self.ws.set_onclose(None);
        self.ws.set_onerror(None);
    }

    pub fn close(&mut self) -> anyhow::Result<()> {
        self.detach();
        let _ = self.ws.close();
        Ok(())
    }
}

impl Drop for JsWss {
    fn drop(&mut self) {
        // Clear handlers before the Closure fields are freed, otherwise a
        // pending WebSocket event would call into dropped closures.
        self.detach();
        let _ = self.ws.close();
    }
}

impl Read for JsWss {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut inbox = self.inbox.borrow_mut();
        if inbox.is_empty() {
            return Err(io::ErrorKind::WouldBlock.into());
        }
        // Bulk copy via the deque's two contiguous slices, not byte-by-byte.
        let n = buf.len().min(inbox.len());
        {
            let (a, b) = inbox.as_slices();
            let na = a.len().min(n);
            buf[..na].copy_from_slice(&a[..na]);
            if na < n {
                buf[na..n].copy_from_slice(&b[..(n - na)]);
            }
        }
        inbox.drain(..n);
        Ok(n)
    }
}

impl Write for JsWss {
    /// Non-blocking write with browser backpressure.
    ///
    /// `WebSocket.send` never refuses data: it queues it in the browser's own
    /// send buffer and there is no `drain` event. Without a high-water mark the
    /// upload phase — whose handler loops `write()` until the phase duration
    /// elapses — would queue the entire upload in memory and never yield to the
    /// JS event loop (so no TIMERESULT would ever be read). Reporting
    /// `WouldBlock` above [`SEND_HIGH_WATER`] gives the handlers the same signal
    /// a full native socket buffer does, and the wasm pump turns it into a tick
    /// of the event loop.
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        match self.ws.ready_state() {
            WebSocket::CONNECTING => return Err(io::ErrorKind::WouldBlock.into()),
            WebSocket::OPEN => {}
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "WebSocket is closed",
                ))
            }
        }
        if self.ws.buffered_amount() >= SEND_HIGH_WATER {
            return Err(io::ErrorKind::WouldBlock.into());
        }
        let n = buf.len().min(MAX_SEND);
        self.ws
            .send_with_u8_array(&buf[..n])
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "WebSocket.send failed"))?;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
