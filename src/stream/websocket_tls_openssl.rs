use anyhow::Result;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use log::debug;
use mio::{net::TcpStream, Interest, Poll, Token};
use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection};
use crate::stream::rustls::RustlsStream;
use sha1::{Digest, Sha1};
use std::io::{self, Read, Write};
use std::net::SocketAddr;
use std::sync::Arc;
use tungstenite::protocol::WebSocketConfig;
use tungstenite::Message;
use tungstenite::WebSocket;

use crate::config::constants::CHUNK_SIZE;

const WS_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

#[derive(Debug)]
pub struct WebSocketTlsClient {
    ws: WebSocket<RustlsStream>,
    handshake_rrequest: Vec<u8>,
    flushed: bool,
    read_buffer: Vec<u8>,
}

impl WebSocketTlsClient {
    pub fn get_greeting(&mut self) -> Vec<u8> {
        self.handshake_rrequest.clone()
    }

    pub fn new(addr: SocketAddr, mut stream: TcpStream, hostname: &str) -> Result<Self> {
        debug!("Connecting to WebSocket server at {}", addr);
        debug!("Creating rustls client config");

        let config = ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(danger::NoCertificateVerification))
            .with_no_client_auth();

        let server_name = ServerName::try_from(hostname.to_string())
            .map_err(|_| anyhow::anyhow!("Invalid hostname"))?;
        let conn = ClientConnection::new(Arc::new(config), server_name)?;

        if let Err(_) = stream.set_nodelay(true) {
            std::thread::sleep(std::time::Duration::from_millis(1000));
            if let Err(e) = stream.set_nodelay(true) {
                debug!("Failed to set TCP_NODELAY: {}", e);
            }
        }

        let mut poll = Poll::new()?;
        let mut events = mio::Events::with_capacity(128);

        poll.registry()
            .register(&mut stream, Token(0), Interest::READABLE | Interest::WRITABLE)?;

        // Wait until TCP is writable (connection established)
        loop {
            poll.poll(&mut events, None)?;
            let mut connection_ready = false;
            for event in events.iter() {
                if event.is_writable() {
                    if let Err(e) = stream.peer_addr() {
                        if e.kind() == io::ErrorKind::NotConnected {
                            debug!("TCP connection not ready yet, waiting...");
                            continue;
                        }
                    }
                    connection_ready = true;
                    break;
                }
            }
            if connection_ready {
                break;
            }
        }

        // TLS handshake via complete_io
        let mut conn = conn;
        loop {
            match conn.complete_io(&mut stream) {
                Ok((_, _)) => {
                    if !conn.is_handshaking() {
                        debug!("TLS handshake completed");
                        break;
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    debug!("Socket not ready, waiting...");
                    poll.poll(&mut events, None)?;
                }
                Err(e) => {
                    debug!("TLS handshake error: {:?}", e);
                    return Err(e.into());
                }
            }
        }

        let key = BASE64.encode(b"dGhlIHNhbXBsZSBub25jZQ==");
        debug!("WebSocket key: {}", key);

        let request = format!(
            "GET / HTTP/1.1\r\n\
             Host: {}\r\n\
             Connection: Upgrade\r\n\
             Upgrade: websocket\r\n\
             Sec-WebSocket-Version: 13\r\n\
             Sec-WebSocket-Key: {}\r\n\
             \r\n",
            addr, key
        );

        let mut tls_stream = RustlsStream::from_connection(conn, stream);

        poll.registry()
            .reregister(tls_stream.get_mut(), Token(0), Interest::WRITABLE)?;

        // Send WebSocket handshake request
        loop {
            poll.poll(&mut events, None)?;
            let mut connection_ready = false;
            for event in events.iter() {
                if event.is_writable() {
                    connection_ready = true;
                    break;
                }
            }
            if connection_ready {
                match tls_stream.write(request.as_bytes()) {
                    Ok(n) => {
                        if n == 0 {
                            debug!("Connection closed during handshake");
                            return Err(anyhow::anyhow!("Connection closed during handshake"));
                        }
                        break;
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => continue,
                    Err(e) => {
                        debug!("WebSocket write error: {}", e);
                        return Err(anyhow::anyhow!("WebSocket write error: {}", e));
                    }
                }
            }
        }

        debug!("WebSocket handshake request sent");
        tls_stream.flush()?;
        poll.registry()
            .reregister(tls_stream.get_mut(), Token(0), Interest::READABLE)?;

        // Read handshake response
        let mut response = Vec::new();
        let mut buffer = [0u8; 2048];
        let mut current_pos = 0;

        loop {
            poll.poll(&mut events, None)?;
            let mut connection_ready = false;
            for event in events.iter() {
                if event.is_readable() {
                    connection_ready = true;
                    break;
                }
            }
            if !connection_ready {
                continue;
            }
            match tls_stream.read(&mut buffer[current_pos..]) {
                Ok(n) => {
                    if n == 0 {
                        debug!("Connection closed during handshake");
                        return Err(anyhow::anyhow!("Connection closed during handshake"));
                    }
                    response.extend_from_slice(&buffer[current_pos..current_pos + n]);
                    current_pos += n;

                    let line = String::from_utf8_lossy(&buffer[..current_pos]);
                    debug!("WebSocket handshake response (partial): {}", line);

                    if line.contains("Sec-WebSocket-Accept:") {
                        break;
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    poll.registry()
                        .reregister(tls_stream.get_mut(), Token(0), Interest::READABLE)?;
                }
                Err(e) => return Err(anyhow::anyhow!("Read error during handshake: {}", e)),
            }
        }

        let response_str = String::from_utf8_lossy(&response);
        debug!("WebSocket handshake response: {}", response_str);

        if !response_str.contains("Upgrade: websocket") {
            return Err(anyhow::anyhow!("Server does not support WebSocket upgrade"));
        }

        let accept_key = if let Some(accept_line) = response_str
            .lines()
            .find(|line| line.starts_with("Sec-WebSocket-Accept:"))
        {
            accept_line
                .split_once(':')
                .map(|(_, value)| value.trim())
                .unwrap_or("")
        } else {
            return Err(anyhow::anyhow!("Missing Sec-WebSocket-Accept header"));
        };

        let expected_accept = Self::generate_accept_key(&key)?;
        if accept_key != expected_accept {
            return Err(anyhow::anyhow!("Invalid Sec-WebSocket-Accept key"));
        }

        let config = WebSocketConfig::default();
        poll.registry().deregister(tls_stream.get_mut())?;

        let ws =
            WebSocket::from_raw_socket(tls_stream, tungstenite::protocol::Role::Client, Some(config));

        debug!("WebSocket created");

        Ok(Self {
            ws,
            handshake_rrequest: request.as_bytes().to_vec(),
            flushed: true,
            read_buffer: vec![],
        })
    }

    fn generate_accept_key(key: &str) -> Result<String> {
        let mut hasher = Sha1::new();
        hasher.update(key.as_bytes());
        hasher.update(WS_GUID.as_bytes());
        let result = hasher.finalize();
        Ok(BASE64.encode(result))
    }

    pub fn close(&mut self) -> Result<()> {
        self.ws
            .close(None)
            .map_err(|e| anyhow::anyhow!("WebSocket close error: {}", e))
    }

    pub fn get_mut(&mut self) -> &mut TcpStream {
        self.ws.get_mut().get_mut()
    }

    pub fn register(&mut self, poll: &Poll, token: Token, interest: Interest) -> Result<()> {
        poll.registry()
            .register(self.get_mut(), token, interest)
            .map_err(|e| anyhow::anyhow!("Failed to register WebSocket: {}", e))
    }

    pub fn reregister(&mut self, poll: &Poll, token: Token, interest: Interest) -> Result<()> {
        poll.registry()
            .reregister(self.get_mut(), token, interest)
            .map_err(|e| anyhow::anyhow!("Failed to reregister WebSocket: {}", e))
    }
}

mod danger {
    use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
    use rustls::pki_types::UnixTime;
    use rustls::pki_types::{CertificateDer, ServerName};
    use rustls::DigitallySignedStruct;

    #[derive(Debug)]
    pub struct NoCertificateVerification;

    impl ServerCertVerifier for NoCertificateVerification {
        fn verify_server_cert(
            &self,
            _end_entity: &CertificateDer,
            _intermediates: &[CertificateDer],
            _server_name: &ServerName,
            _scts: &[u8],
            _now: UnixTime,
        ) -> Result<ServerCertVerified, rustls::Error> {
            Ok(ServerCertVerified::assertion())
        }

        fn verify_tls12_signature(
            &self,
            _message: &[u8],
            _cert: &CertificateDer,
            _dss: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, rustls::Error> {
            Ok(HandshakeSignatureValid::assertion())
        }

        fn verify_tls13_signature(
            &self,
            _message: &[u8],
            _cert: &CertificateDer,
            _dss: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, rustls::Error> {
            Ok(HandshakeSignatureValid::assertion())
        }

        fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
            vec![
                rustls::SignatureScheme::RSA_PSS_SHA256,
                rustls::SignatureScheme::RSA_PSS_SHA384,
                rustls::SignatureScheme::RSA_PSS_SHA512,
                rustls::SignatureScheme::RSA_PKCS1_SHA256,
                rustls::SignatureScheme::RSA_PKCS1_SHA384,
                rustls::SignatureScheme::RSA_PKCS1_SHA512,
                rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
                rustls::SignatureScheme::ECDSA_NISTP384_SHA384,
                rustls::SignatureScheme::ECDSA_NISTP521_SHA512,
                rustls::SignatureScheme::ED25519,
            ]
        }
    }
}

impl Read for WebSocketTlsClient {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut current_pos = 0;

        if !self.read_buffer.is_empty() {
            let take = self.read_buffer.len().min(buf.len());
            buf[..take].copy_from_slice(&self.read_buffer[..take]);
            self.read_buffer.drain(..take);
            current_pos = take;
            if current_pos == buf.len() {
                return Ok(current_pos);
            }
        }

        match self.ws.read() {
            Ok(Message::Binary(data)) => {
                let space = buf.len() - current_pos;
                if data.len() <= space {
                    let len = data.len();
                    buf[current_pos..current_pos + len].copy_from_slice(&data[..len]);
                    Ok(current_pos + len)
                } else {
                    buf[current_pos..].copy_from_slice(&data[..space]);
                    self.read_buffer.extend_from_slice(&data[space..]);
                    Ok(buf.len())
                }
            }
            Ok(Message::Text(text)) => {
                let bytes = text.as_bytes();
                let space = buf.len() - current_pos;
                if bytes.len() <= space {
                    let len = bytes.len();
                    buf[current_pos..current_pos + len].copy_from_slice(&bytes[..len]);
                    Ok(current_pos + len)
                } else {
                    buf[current_pos..].copy_from_slice(&bytes[..space]);
                    self.read_buffer.extend_from_slice(&bytes[space..]);
                    Ok(buf.len())
                }
            }
            Ok(Message::Close(_)) => Ok(0),
            Ok(_) => Ok(0),
            Err(e) => match e {
                tungstenite::Error::Io(io_err)
                    if io_err.kind() == std::io::ErrorKind::WouldBlock =>
                {
                    if current_pos > 0 {
                        return Ok(current_pos);
                    }
                    return Err(io::Error::new(io::ErrorKind::WouldBlock, "WouldBlock"));
                }
                _ => return Err(io::Error::new(io::ErrorKind::Other, e)),
            },
        }
    }
}

impl Write for WebSocketTlsClient {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.flushed {
            let message = if buf.len() < 2 || buf.len() > (CHUNK_SIZE - 3) {
                tokio_tungstenite::tungstenite::Message::Binary(buf.to_vec())
            } else {
                tokio_tungstenite::tungstenite::Message::Text(
                    String::from_utf8_lossy(buf).to_string(),
                )
            };
            match self.ws.write(message) {
                Ok(_) => {
                    self.flushed = false;
                    return self.write(buf);
                }
                Err(e) => match e {
                    tungstenite::Error::Io(io_err)
                        if io_err.kind() == std::io::ErrorKind::WouldBlock =>
                    {
                        self.flushed = false;
                        Err(io::Error::new(io::ErrorKind::WouldBlock, "WouldBlock"))
                    }
                    _ => {
                        debug!("WebSocket write error: {}", e);
                        return Err(io::Error::new(io::ErrorKind::Other, e));
                    }
                },
            }
        } else {
            match self.ws.flush() {
                Ok(_) => {
                    self.flushed = true;
                    return Ok(buf.len());
                }
                Err(e) => {
                    match e {
                        tungstenite::Error::Io(io_err)
                            if io_err.kind() == std::io::ErrorKind::WouldBlock =>
                        {
                            // debug!("WouldBlock flush {}", io_err.to_string());
                            return Err(io::Error::new(io::ErrorKind::WouldBlock, "WouldBlock"));
                        }
                        _ => {
                            debug!("WebSocket flush error: {}", e);
                            return Err(io::Error::new(io::ErrorKind::Other, e));
                        }
                    }
                }
            }
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self.ws.flush() {
            Ok(_) => {
                self.flushed = true;
                // let a = self.ws.close(None);
                return Ok(());
            }
            Err(e) => match e {
                tungstenite::Error::Io(io_err)
                    if io_err.kind() == std::io::ErrorKind::WouldBlock =>
                {
                    debug!("WouldBlock flush {}", io_err.to_string());
                    return Err(io::Error::new(io::ErrorKind::WouldBlock, "WouldBlock"));
                }
                _ => return Err(io::Error::new(io::ErrorKind::Other, e)),
            },
        }
    }
}