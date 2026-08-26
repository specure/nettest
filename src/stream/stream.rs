// ===========================================================================
// Native transport variants (raw TCP / TLS / native WebSocket). Not available
// in a browser WASM build.
// ===========================================================================
#[cfg(not(target_arch = "wasm32"))]
mod native {
    use crate::reactor::{Interest, Poll, TcpStream, Token};
    use anyhow::{Ok, Result};
    use log::{debug, info};
    use std::io::{self, Read, Write};
    use std::net::SocketAddr;
    use std::path::Path;

    use crate::client::constants::RMBT_UPGRADE_REQUEST;
    use crate::stream::{
        rustls::RustlsStream, rustls_server::RustlsServerStream,
        websocket::WebSocketClient, websocket_rustls_server::WebSocketRustlsServerStream,
        websocket_tls::WebSocketTlsClient,
    };
    use crate::utils::websocket::Handshake;

    #[derive(Debug)]
    pub enum Stream {
        Tcp(TcpStream),
        WebSocket(WebSocketClient),
        Rustls(RustlsStream),
        RustlsServer(RustlsServerStream),
        WebSocketTls(WebSocketTlsClient),
        WebSocketRustlsServer(WebSocketRustlsServerStream),
    }

    impl Stream {
        pub fn new_tcp(addr: SocketAddr) -> Result<Self> {
            debug!("Connecting to TCP at {}", addr);
            let stream = TcpStream::connect(addr)?;
            if let Err(_) = stream.set_nodelay(true) {
                std::thread::sleep(std::time::Duration::from_millis(1000));
                if let Err(e) = stream.set_nodelay(true) {
                    info!("Failed to set TCP_NODELAY: {}", e);
                }
            }
            let std = Self::Tcp(stream);
            Ok(std)
        }

        pub fn return_type(&self) -> &str {
            match self {
                Stream::Tcp(_) => "Tcp",
                Stream::WebSocket(_) => "WebSocket",
                Stream::Rustls(_) => "Rustls",
                Stream::WebSocketTls(_) => "WebSocketTls",
                Stream::RustlsServer(_) => "RustlsServer",
                Stream::WebSocketRustlsServer(_) => "WebSocketRustlsServer",
            }
        }

        pub fn upgrade_to_websocket(self) -> Result<Stream> {
            match self {
                Stream::Tcp(stream) => {
                    let stream = WebSocketClient::new_server(stream)?;
                    Ok(Stream::WebSocket(stream))
                }
                Stream::RustlsServer(stream) => {
                    let stream = WebSocketRustlsServerStream::from_rustls_server_stream(stream)?;
                    Ok(Stream::WebSocketRustlsServer(stream))
                }
                _ => Ok(self),
            }
        }

        pub fn finish_server_handshake(&mut self, handshake: Handshake) -> Result<()> {
            match self {
                Stream::WebSocket(stream) => stream.finish_server_handshake(handshake),
                Stream::WebSocketRustlsServer(stream) => stream.finish_server_handshake(handshake),
                _ => Ok(()),
            }
        }

        pub fn new_websocket(addr: SocketAddr) -> Result<Self> {
            let ws_client = WebSocketClient::new(addr)?;
            Ok(Self::WebSocket(ws_client))
        }

        pub fn new_rustls(
            addr: SocketAddr,
            cert_path: Option<&Path>,
            key_path: Option<&Path>,
        ) -> Result<Self> {
            debug!("Creating Rustls stream {:?}", addr);
            let stream = RustlsStream::new(addr, cert_path, key_path)?;
            Ok(Self::Rustls(stream))
        }

        pub fn new_rustls_server(
            stream: TcpStream,
            cert_path: String,
            key_path: String,
        ) -> Result<Self> {
            let stream = RustlsServerStream::new(stream, cert_path, key_path)?;
            Ok(Self::RustlsServer(stream))
        }

        pub fn close(&mut self) -> Result<()> {
            match self {
                Stream::Tcp(_) => Ok(()),
                Stream::WebSocket(stream) => stream.close(),
                Stream::Rustls(_) => Ok(()),
                Stream::WebSocketTls(stream) => stream.close(),
                Stream::RustlsServer(_) => Ok(()),
                Stream::WebSocketRustlsServer(_) => Ok(()),
            }
        }

        pub fn get_greeting(&mut self) -> Vec<u8> {
            match self {
                Stream::Tcp(_) => RMBT_UPGRADE_REQUEST.as_bytes().to_vec(),
                Stream::WebSocket(_) => RMBT_UPGRADE_REQUEST.as_bytes().to_vec(),
                Stream::Rustls(_) => RMBT_UPGRADE_REQUEST.as_bytes().to_vec(),
                Stream::WebSocketTls(_) => RMBT_UPGRADE_REQUEST.as_bytes().to_vec(),
                Stream::RustlsServer(_) => RMBT_UPGRADE_REQUEST.as_bytes().to_vec(),
                Stream::WebSocketRustlsServer(_) => RMBT_UPGRADE_REQUEST.as_bytes().to_vec(),
            }
        }


        pub fn new_websocket_tls(addr: SocketAddr) -> Result<Self> {
            let stream1 = TcpStream::connect(addr)?;
            if let Err(_) = stream1.set_nodelay(true) {
                std::thread::sleep(std::time::Duration::from_millis(1000));
                if let Err(e) = stream1.set_nodelay(true) {
                    debug!("Failed to set TCP_NODELAY: {}", e);
                }
            }
            let stream = WebSocketTlsClient::new(addr, stream1, "localhost")?;
            Ok(Self::WebSocketTls(stream))
        }

        pub fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            match self {
                Stream::Tcp(stream) => stream.read(buf),
                Stream::WebSocket(stream) => stream.read(buf),
                Stream::Rustls(stream) => stream.read(buf),
                Stream::WebSocketTls(stream) => stream.read(buf),
                Stream::RustlsServer(stream) => stream.read(buf),
                Stream::WebSocketRustlsServer(stream) => stream.read(buf),
            }
        }

        /// Consume up to `max` incoming bytes without handing them out, and
        /// report the last byte consumed.
        ///
        /// The RMBT download only needs a byte count and the terminator byte
        /// that ends each chunk, never the payload. A browser transport can do
        /// that by advancing a cursor; here there is no buffered stream to
        /// advance, so the bytes are read into a scratch buffer that is reused
        /// across calls — the same single copy `read` would have done, so this
        /// path is no slower than before.
        pub fn consume(&mut self, max: usize) -> io::Result<(usize, u8)> {
            thread_local! {
                static SCRATCH: std::cell::RefCell<Vec<u8>> =
                    std::cell::RefCell::new(vec![0u8; 256 * 1024]);
            }
            SCRATCH.with(|scratch| -> io::Result<(usize, u8)> {
                let mut scratch = scratch.borrow_mut();
                let want = max.min(scratch.len());
                let n = self.read(&mut scratch[..want])?;
                if n == 0 {
                    return std::result::Result::Ok((0, 0));
                }
                std::result::Result::Ok((n, scratch[n - 1]))
            })
        }

        pub fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            match self {
                Stream::Tcp(stream) => stream.write(buf),
                Stream::WebSocket(stream) => stream.write(buf),
                Stream::Rustls(stream) => stream.write(buf),
                Stream::WebSocketTls(stream) => stream.write(buf),
                Stream::RustlsServer(stream) => stream.write(buf),
                Stream::WebSocketRustlsServer(stream) => stream.write(buf),
            }
        }

        pub fn register(&mut self, poll: &Poll, token: Token, interest: Interest) -> Result<()> {
            match self {
                Stream::Tcp(stream) => {
                    poll.registry().register(stream, token, interest)?;
                }
                Stream::WebSocket(stream) => {
                    stream.register(poll, token, interest)?;
                }
                Stream::Rustls(stream) => {
                    stream.register(poll, token, interest)?;
                }
                Stream::WebSocketTls(stream) => {
                    stream.register(poll, token, interest)?;
                }
                Stream::RustlsServer(stream) => {
                    stream.register(poll, token, interest)?;
                }
                Stream::WebSocketRustlsServer(stream) => {
                    stream.register(poll, token, interest)?;
                }
            }
            Ok(())
        }

        pub fn flush(&mut self) -> io::Result<()> {
            match self {
                Stream::Tcp(stream) => stream.flush(),
                Stream::WebSocket(stream) => stream.flush(),
                Stream::Rustls(stream) => stream.flush(),
                Stream::WebSocketTls(stream) => stream.flush(),
                Stream::RustlsServer(stream) => stream.flush(),
                Stream::WebSocketRustlsServer(stream) => stream.flush(),
            }
        }

        pub fn reregister(
            &mut self,
            poll: &Poll,
            token: Token,
            interest: Interest,
        ) -> io::Result<()> {
            match self {
                Stream::Tcp(stream) => poll
                    .registry()
                    .reregister(stream, token, interest)
                    .map_err(|e| io::Error::new(io::ErrorKind::Other, e)),
                Stream::WebSocket(stream) => stream
                    .reregister(poll, token, interest)
                    .map_err(|e| io::Error::new(io::ErrorKind::Other, e)),
                Stream::Rustls(stream) => stream
                    .reregister(poll, token, interest)
                    .map_err(|e| io::Error::new(io::ErrorKind::Other, e)),
                Stream::WebSocketTls(stream) => stream
                    .reregister(poll, token, interest)
                    .map_err(|e| io::Error::new(io::ErrorKind::Other, e)),
                Stream::RustlsServer(stream) => stream
                    .reregister(poll, token, interest)
                    .map_err(|e| io::Error::new(io::ErrorKind::Other, e)),
                Stream::WebSocketRustlsServer(stream) => stream
                    .reregister(poll, token, interest)
                    .map_err(|e| io::Error::new(io::ErrorKind::Other, e)),
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use native::Stream;

// ===========================================================================
// Browser WASM transport: a single `Js` variant wrapping a WebSocket. Same
// method surface as the native `Stream`, so the shared RMBT handlers compile and
// run over it unchanged. TLS/handshake/upgrade are handled by the browser, so
// the corresponding methods are no-ops here.
// ===========================================================================
#[cfg(target_arch = "wasm32")]
mod wasm {
    use crate::reactor::{Interest, Poll, Token};
    use crate::stream::js_wss::JsWss;
    use anyhow::Result;
    use std::io::{Read, Write};

    #[derive(Debug)]
    pub enum Stream {
        Js(JsWss),
    }

    impl Stream {
        pub fn new_js(url: &str) -> Result<Self> {
            let ws = JsWss::connect(url).map_err(|e| anyhow::anyhow!("ws connect failed: {:?}", e))?;
            Ok(Stream::Js(ws))
        }

        pub fn inner(&self) -> &JsWss {
            match self {
                Stream::Js(s) => s,
            }
        }

        pub fn return_type(&self) -> &str {
            "Js"
        }

        /// Browser already performed the WebSocket handshake — no RMBT upgrade.
        pub fn get_greeting(&mut self) -> Vec<u8> {
            Vec::new()
        }

        pub fn close(&mut self) -> Result<()> {
            match self {
                Stream::Js(s) => s.close(),
            }
        }

        pub fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            match self {
                Stream::Js(s) => s.read(buf),
            }
        }

        /// Consume up to `max` incoming bytes without copying them out, and
        /// report the last byte consumed — for the browser this is a cursor
        /// advance over the queued messages, so the download phase moves its
        /// payload with a single copy (JS memory into wasm) and no second one.
        pub fn consume(&mut self, max: usize) -> std::io::Result<(usize, u8)> {
            match self {
                Stream::Js(s) => s.consume(max),
            }
        }

        pub fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            match self {
                Stream::Js(s) => s.write(buf),
            }
        }

        pub fn flush(&mut self) -> std::io::Result<()> {
            match self {
                Stream::Js(s) => s.flush(),
            }
        }

        pub fn register(&mut self, poll: &Poll, token: Token, interest: Interest) -> Result<()> {
            match self {
                Stream::Js(s) => {
                    s.register(poll, token, interest)?;
                    Ok(())
                }
            }
        }

        pub fn reregister(
            &mut self,
            poll: &Poll,
            token: Token,
            interest: Interest,
        ) -> std::io::Result<()> {
            match self {
                Stream::Js(s) => s.reregister(poll, token, interest),
            }
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub use wasm::Stream;
