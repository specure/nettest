use anyhow::Result;
use log::{debug, info};
use mio::{net::TcpStream, Interest, Poll, Token};
use sha1::{Digest, Sha1};
use openssl::ssl::{Ssl, SslContext, SslMethod, SslMode, SslOptions, SslStream};
use tungstenite::protocol::WebSocketConfig;
use tungstenite::Message;
use tungstenite::WebSocket;
use std::io::{self, Read, Write};
use std::net::SocketAddr;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
const WS_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

#[derive(Debug)]
pub struct WebSocketTlsClient {
    ws: WebSocket<SslStream<TcpStream>>,
    handshake_rrequest: Vec<u8>,
    flushed: bool,
}

impl WebSocketTlsClient {
    pub fn get_greeting(&mut self) -> Vec<u8> {
        self.handshake_rrequest.clone()
    }

    pub fn new(addr: SocketAddr,stream: TcpStream, hostname: &str) -> Result<Self> {
       

        debug!("Connecting to WebSocket server at {}", addr);
        debug!("Creating SSL context");
        let mut ctx = SslContext::builder(SslMethod::tls_client())?;
        debug!("Setting verify mode to NONE");

        ctx.set_verify(openssl::ssl::SslVerifyMode::NONE);
        ctx.set_mode(SslMode::RELEASE_BUFFERS);
        ctx.set_options(SslOptions::NO_COMPRESSION);
        ctx.set_ciphersuites("TLS_AES_128_GCM_SHA256")?;

        // Устанавливаем версии протокола
        ctx.set_max_proto_version(Some(openssl::ssl::SslVersion::TLS1_3))?;
        ctx.set_min_proto_version(Some(openssl::ssl::SslVersion::TLS1_2))?;

        let mut ssl = Ssl::new(&ctx.build())?;
        ssl.set_hostname(hostname)?;

        debug!("Creating SSL stream");
        let mut stream = SslStream::new(ssl, stream)?;
        debug!("SSL stream created");

        // Устанавливаем неблокирующий режим
        let tcp_stream = stream.get_mut();
        tcp_stream.set_nodelay(true)?;

        // Создаем Poll для ожидания событий
        let mut poll = Poll::new()?;
        let mut events = mio::Events::with_capacity(128);

        // Регистрируем сокет для чтения и записи
        poll.registry().register(
            stream.get_mut(),
            Token(0),
            Interest::READABLE | Interest::WRITABLE,
        )?;

        // Ждем, пока TCP соединение будет установлено
        loop {
            poll.poll(&mut events, None)?;
            let mut connection_ready = false;

            for event in events.iter() {
                if event.is_writable() {
                    // Проверяем, что соединение действительно установлено
                    if let Err(e) = stream.get_ref().peer_addr() {
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

        // Теперь можно начинать TLS handshake
        loop {
            match stream.connect() {
                Ok(_) => {
                    debug!("Handshake completed");
                    break;
                }
                Err(e) => {
                    // Проверяем на вложенную ошибку WouldBlock
                    if let Some(io_error) = e.io_error() {
                        if io_error.kind() == io::ErrorKind::WouldBlock {
                            debug!("Socket not ready, waiting...");
                            poll.poll(&mut events, None)?;
                            continue;
                        }
                    }
                    debug!("Error during handshake: {:?}", e);
                    return Err(e.into());
                }
            }
        }

        // Generate WebSocket key
        let key = BASE64.encode(b"dGhlIHNhbXBsZSBub25jZQ==");


        debug!("WebSocket key: {}", key);

        // Create WebSocket handshake request
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

        // Создаем Poll для ожидания событий
        let mut poll = Poll::new()?;
        let mut events = mio::Events::with_capacity(128);

        // Регистрируем сокет для чтения и записи
        poll.registry()
            .register(stream.get_mut(), Token(0), Interest::WRITABLE)?;

        // Send handshake request
        loop {
            poll.poll(&mut events, None)?;
            let mut connection_ready = false;

            for event in events.iter() {
                if event.is_writable() {
                    connection_ready = true;
                }
            }

            if connection_ready {
                match stream.write(request.as_bytes()) {
                    Ok(n) => {
                        if n == 0 {
                            debug!("Connection closed during handshake");
                            return Err(anyhow::anyhow!("Connection closed during handshake"));
                        }
                        break;
                    }
                    Err(e) => {
                        debug!("WebSocket write error: {}", e);
                        return Err(anyhow::anyhow!("WebSocket write error: {}", e));
                    }
                }
            }
        }

        debug!("WebSocket handshake request: {}", request);
        poll.registry()
            .reregister(stream.get_mut(), Token(0), Interest::READABLE)?;

        // Read handshake response
        let mut response = Vec::new();
        let mut buffer = [0u8; 1024];
            poll.poll(&mut events, None)?;

        loop {
            poll.poll(&mut events, None)?;

            let connection_ready = true;

            // for event in events.iter() {
            //     if event.is_readable() {
            //         connection_ready = true;
            //     }
            // }

            if connection_ready {
                match stream.read(&mut buffer) {
                
                    Ok(n) => {
                        if n == 0 {
                            debug!("Connection closed during handshake");
                            return Err(anyhow::anyhow!("Connection closed during handshake"));
                        }
                        response.extend_from_slice(&buffer[..n]);

                        let line = String::from_utf8_lossy(&buffer);

                        debug!(
                            "WebSocket handshake response: {}",
                            String::from_utf8_lossy(&buffer)
                        );

                        if line.contains("RMBT") {
                            debug!(
                                "WebSocket handshake response FOUND: {}",
                                String::from_utf8_lossy(&response)
                            );
                            break;
                        } else {
                            poll.registry()
                            .reregister(stream.get_mut(), Token(0), Interest::READABLE)?;
                            debug!(
                                "WebSocket handshake response: {}",
                                String::from_utf8_lossy(&response)
                            );
                        }
                    }
                    Err(e) => {
                        if e.to_string().contains("WouldBlock") {
                            break;
                        }
                    }
                }
            }
        }

        let response_str = String::from_utf8_lossy(&response);
        debug!("WebSocket handshake response: {}", response_str);

        if !response_str.contains("Upgrade: websocket") {
            return Err(anyhow::anyhow!("Server does not support WebSocket upgrade"));
        }

        // Extract Sec-WebSocket-Accept header
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

        // Verify accept key
        let expected_accept = Self::generate_accept_key(&key)?;
        if accept_key != expected_accept {
            return Err(anyhow::anyhow!("Invalid Sec-WebSocket-Accept key"));
        }

        info!("WebSocket handshake completed successfully");

        // Create WebSocket with the established connection
        let config = WebSocketConfig::default();
        // config.max_write_buffer_size = MAX_CHUNK_SIZE as usize;
        let ws =
            WebSocket::from_raw_socket(stream, tungstenite::protocol::Role::Client, Some(config));

        Ok(Self {
            ws,
            handshake_rrequest: request.as_bytes().to_vec(),
            flushed: true,
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

impl Read for WebSocketTlsClient {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self.ws.read() {
            Ok(Message::Binary(data)) => {
                debug!("WebSocket binary: {} bytes", data.len());
                let len = data.len().min(buf.len());
                buf[..len].copy_from_slice(&data[..len]);
                Ok(len)
            }
            Ok(Message::Text(text)) => {
                debug!("WebSocket text: {} bytes", text.len());
                let bytes = text.as_bytes();
                let len = bytes.len().min(buf.len());
                buf[..len].copy_from_slice(&bytes[..len]);
                Ok(len)
            }
            Ok(Message::Close(_)) => {
                debug!("WebSocket close message");
                Ok(0)
            }
            Ok(_) => {
                debug!("WebSocket other message");
                Ok(0)
            }
            Err(e) => match e {
                tungstenite::Error::Io(io_err)
                    if io_err.kind() == std::io::ErrorKind::WouldBlock =>
                {
                    Err(io::Error::new(io::ErrorKind::WouldBlock, "WouldBlock"))
                }
                _ => Err(io::Error::new(io::ErrorKind::Other, e)),
            },
        }
    }
}

impl Write for WebSocketTlsClient {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.flushed {
            debug!("WebSocket write");
            let message = Message::Binary(buf.to_vec().into());

            match self.ws.write(message) {
                Ok(_) => {
                    self.flushed = false;
                    return self.write(buf);
                }
                Err(e) => match e {
                    tungstenite::Error::Io(io_err)
                        if io_err.kind() == std::io::ErrorKind::WouldBlock =>
                    {
                        debug!("WouldBlock WRITE!!!! {}", io_err.to_string());
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
            debug!("WebSocket flush");
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

    // fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
    //     let message = Message::Binary(buf.to_vec().into());
    //     match self.ws.send(message) {
    //         Ok(_) => {

    //             // self.flushed = false;
    //             return Ok(buf.len());
    //         }
    //         Err(e) => match e {
    //             tungstenite::Error::Io(io_err)
    //                 if io_err.kind() == std::io::ErrorKind::WouldBlock =>
    //             {
    //                 debug!("WouldBlock WRITE!!!! {}", io_err.to_string());
    //                 // self.flushed = false;
    //                 Err(io::Error::new(io::ErrorKind::WouldBlock, "WouldBlock"))
    //             }
    //             _ => {
    //                 debug!("WebSocket write error: {}", e);
    //                 return Err(io::Error::new(io::ErrorKind::Other, e));
    //             }
    //         },
    //     }
    // }

    fn flush(&mut self) -> io::Result<()> {
        match self.ws.flush() {
            Ok(_) => {
                self.flushed = true;
                debug!("WebSocket flush success");
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
