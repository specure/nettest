//! Browser WebSocket transport for the `Stream` interface (wasm only).
//!
//! `JsWss` is the wasm sibling of the native `mio::net::TcpStream` /
//! `WebSocketClient` variants: a **non-blocking** byte stream backed by a
//! browser `WebSocket`. Reads drain bytes buffered by `onmessage` and return
//! `WouldBlock` when empty — exactly the contract the RMBT handlers already
//! expect from a non-blocking socket. Writes go straight to `WebSocket.send`.
//!
//! `register`/`reregister` don't touch any OS poll (there is none in a browser);
//! they just record the interest so the wasm pump (the JS-driven event loop that
//! replaces `poll.poll()`) knows whether to feed reads or drive writes.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::rc::Rc;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{BinaryType, MessageEvent, WebSocket};

use crate::reactor::{Interest, Poll, Token};

type Inbox = Rc<RefCell<VecDeque<u8>>>;

pub struct JsWss {
    ws: WebSocket,
    inbox: Inbox,
    interest: Rc<Cell<Interest>>,
    opened: Rc<Cell<bool>>,
    // Kept alive for the socket's lifetime.
    _onmessage: Closure<dyn FnMut(MessageEvent)>,
    _onopen: Closure<dyn FnMut(JsValue)>,
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
        let inbox_cb = inbox.clone();
        let onmessage = Closure::wrap(Box::new(move |e: MessageEvent| {
            if let Ok(ab) = e.data().dyn_into::<js_sys::ArrayBuffer>() {
                let arr = js_sys::Uint8Array::new(&ab);
                let mut v = vec![0u8; arr.length() as usize];
                arr.copy_to(&mut v);
                inbox_cb.borrow_mut().extend(v);
            } else if let Some(s) = e.data().as_string() {
                inbox_cb.borrow_mut().extend(s.into_bytes());
            }
        }) as Box<dyn FnMut(MessageEvent)>);
        ws.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));

        let opened = Rc::new(Cell::new(false));
        let opened_cb = opened.clone();
        let onopen = Closure::wrap(Box::new(move |_e: JsValue| {
            opened_cb.set(true);
        }) as Box<dyn FnMut(JsValue)>);
        ws.set_onopen(Some(onopen.as_ref().unchecked_ref()));

        Ok(JsWss {
            ws,
            inbox,
            interest: Rc::new(Cell::new(Interest::READABLE)),
            opened,
            _onmessage: onmessage,
            _onopen: onopen,
        })
    }

    pub fn is_open(&self) -> bool {
        self.opened.get()
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

    pub fn register(&mut self, _poll: &Poll, _token: Token, interest: Interest) -> io::Result<()> {
        self.interest.set(interest);
        Ok(())
    }

    pub fn reregister(&mut self, _poll: &Poll, _token: Token, interest: Interest) -> io::Result<()> {
        self.interest.set(interest);
        Ok(())
    }

    pub fn close(&mut self) -> anyhow::Result<()> {
        let _ = self.ws.close();
        Ok(())
    }
}

impl Read for JsWss {
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

impl Write for JsWss {
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
