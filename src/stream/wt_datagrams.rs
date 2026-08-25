//! Browser WebTransport datagram transport (wasm only) — the QoS sibling of
//! [`crate::stream::js_wss::JsWss`].
//!
//! The RMBT control channel stays on the WebSocket; this carries the jitter /
//! packet-loss trains, because QUIC datagrams are the only thing a browser can
//! send that is *not* retransmitted — which is precisely what makes loss and
//! jitter measurable.
//!
//! The bindings are hand-written rather than taken from `web-sys`, whose
//! `WebTransport` sits behind the `web_sys_unstable_apis` cfg: an extra
//! `RUSTFLAGS` requirement for every build of this crate, in exchange for types
//! we would wrap anyway. What we need is small and stable in the spec.

use js_sys::{Array, Object, Reflect, Uint8Array};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_name = WebTransport)]
    type JsWebTransport;

    #[wasm_bindgen(constructor, js_class = "WebTransport", catch)]
    fn new(url: &str) -> Result<JsWebTransport, JsValue>;

    #[wasm_bindgen(constructor, js_class = "WebTransport", catch)]
    fn new_with_options(url: &str, options: &JsValue) -> Result<JsWebTransport, JsValue>;

    #[wasm_bindgen(method, getter)]
    fn ready(this: &JsWebTransport) -> js_sys::Promise;

    #[wasm_bindgen(method, getter)]
    fn closed(this: &JsWebTransport) -> js_sys::Promise;

    #[wasm_bindgen(method, getter)]
    fn datagrams(this: &JsWebTransport) -> JsValue;

    #[wasm_bindgen(method, js_name = close)]
    fn close(this: &JsWebTransport);
}

/// A WebTransport session reduced to what the QoS phase needs: send a datagram,
/// await the next one, close.
pub struct WtDatagrams {
    transport: JsWebTransport,
    writer: JsValue,
    reader: JsValue,
}

impl WtDatagrams {
    /// Open a session to `url`.
    ///
    /// `cert_hash_base64` is the server's leaf certificate digest, used only
    /// when `self_signed` — `serverCertificateHashes` accepts short-lived
    /// P-256 certificates *only*, so passing it for a normally trusted chain
    /// would fail a connection that plain verification accepts.
    pub async fn connect(
        url: &str,
        cert_hash_base64: Option<&str>,
        self_signed: bool,
    ) -> Result<WtDatagrams, JsValue> {
        if !supported() {
            return Err(JsValue::from_str("WebTransport is not available"));
        }
        let transport = match (self_signed, cert_hash_base64) {
            (true, Some(hash)) => {
                let bytes = decode_base64(hash)
                    .ok_or_else(|| JsValue::from_str("malformed certificate hash"))?;
                let entry = Object::new();
                Reflect::set(&entry, &"algorithm".into(), &"sha-256".into())?;
                Reflect::set(&entry, &"value".into(), &Uint8Array::from(&bytes[..]).into())?;
                let hashes = Array::new();
                hashes.push(&entry);
                let options = Object::new();
                Reflect::set(&options, &"serverCertificateHashes".into(), &hashes)?;
                JsWebTransport::new_with_options(url, &options)?
            }
            _ => JsWebTransport::new(url)?,
        };

        JsFuture::from(transport.ready()).await?;

        let datagrams = transport.datagrams();
        let writable = Reflect::get(&datagrams, &"writable".into())?;
        let readable = Reflect::get(&datagrams, &"readable".into())?;
        let writer = call_method(&writable, "getWriter", &[])?;
        let reader = call_method(&readable, "getReader", &[])?;

        Ok(WtDatagrams { transport, writer, reader })
    }

    /// Queue one datagram. QUIC may drop it — that is the point of the test.
    pub async fn send(&self, payload: &[u8]) -> Result<(), JsValue> {
        let array = Uint8Array::from(payload);
        let promise = call_method(&self.writer, "write", &[array.into()])?;
        JsFuture::from(js_sys::Promise::from(promise)).await?;
        Ok(())
    }

    /// Await the next datagram. `Ok(None)` means the stream ended.
    pub async fn receive(&self) -> Result<Option<Vec<u8>>, JsValue> {
        let promise = call_method(&self.reader, "read", &[])?;
        let result = JsFuture::from(js_sys::Promise::from(promise)).await?;
        if Reflect::get(&result, &"done".into())?.as_bool().unwrap_or(false) {
            return Ok(None);
        }
        let value = Reflect::get(&result, &"value".into())?;
        if value.is_undefined() || value.is_null() {
            return Ok(None);
        }
        Ok(Some(Uint8Array::new(&value).to_vec()))
    }

    pub fn close(&self) {
        self.transport.close();
    }
}

impl Drop for WtDatagrams {
    fn drop(&mut self) {
        self.transport.close();
    }
}

/// Is `WebTransport` available in this browser at all?
pub fn supported() -> bool {
    Reflect::get(&js_sys::global(), &"WebTransport".into())
        .map(|v| v.is_function())
        .unwrap_or(false)
}

fn call_method(target: &JsValue, name: &str, args: &[JsValue]) -> Result<JsValue, JsValue> {
    let function: js_sys::Function = Reflect::get(target, &JsValue::from_str(name))?
        .dyn_into()
        .map_err(|_| JsValue::from_str(&format!("{name} is not a function")))?;
    let argv = Array::new();
    for arg in args {
        argv.push(arg);
    }
    Reflect::apply(&function, target, &argv)
}

/// Decode base64 through the browser's own `atob`, avoiding a second base64
/// implementation in the wasm binary.
fn decode_base64(input: &str) -> Option<Vec<u8>> {
    let atob: js_sys::Function = Reflect::get(&js_sys::global(), &"atob".into())
        .ok()?
        .dyn_into()
        .ok()?;
    let decoded = atob
        .call1(&JsValue::NULL, &JsValue::from_str(input))
        .ok()?
        .as_string()?;
    Some(decoded.chars().map(|c| c as u32 as u8).collect())
}
