use anyhow::Result;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use log::{debug};
use mio::{net::TcpStream, Interest, Poll, Token};
use sha1::{Digest, Sha1};
use std::io::{self, Read, Write};
use std::net::SocketAddr;
use tungstenite::protocol::WebSocketConfig;
use tungstenite::{protocol::WebSocket, Message};

use crate::config::constants::CHUNK_SIZE;
use crate::tokio_server::utils::websocket::{generate_handshake_response, Handshake};


const WS_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

#[derive(Debug)]
pub struct WebSocketClient {
    ws: WebSocket<TcpStream>,
    handshake_rrequest: Vec<u8>,
    flushed: bool,
}

impl WebSocketClient {
    pub fn get_greeting(&mut self) -> Vec<u8> {
        self.handshake_rrequest.clone()
    }

    pub fn finish_server_handshake(&mut self, handshake: Handshake) -> Result<()> {
        let response = generate_handshake_response(&handshake).unwrap();
        self.write_all(response.as_bytes()).unwrap();
        Ok(())
    }


    pub fn new_server(stream: TcpStream) -> Result<Self> {
        let ws = WebSocket::from_raw_socket(stream, tungstenite::protocol::Role::Server, None);
        
        Ok(Self {
            ws,
            handshake_rrequest: vec![],
            flushed: true,
        })
    }

    pub fn new(addr: SocketAddr) -> Result<Self> {
        debug!("Connecting to WebSocket server WS at {}", addr);
        let mut stream = TcpStream::connect(addr)?;
        stream.set_nodelay(true)?;

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
            .register(&mut stream, Token(0), Interest::WRITABLE)?;

        debug!("WebSocket handshake request: {}", request);

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
            .reregister(&mut stream, Token(0), Interest::READABLE)?;

        // Read handshake response
        let mut response = Vec::new();
        let mut buffer = [0u8; 1024];


        loop {
            poll.poll(&mut events, None)?;
            let mut connection_ready = false;


            for event in events.iter() {
                if event.is_readable() {
                    connection_ready = true;
                }
            }

            if connection_ready {
                match stream.read(&mut buffer) {
                    Ok(n) => {
                        if n == 0 {
                            debug!("Connection closed during handshake");
                            return Err(anyhow::anyhow!("Connection closed during handshake"));
                        }
                        debug!("WebSocket handshake response loop 3");
                        response.extend_from_slice(&buffer[..n]);

                        let line = String::from_utf8_lossy(&buffer);

                        debug!(
                            "WebSocket handshake response: {}",
                            String::from_utf8_lossy(&buffer)
                        );

                        if line.contains("ACCEPT TOKEN QUIT\n") {
                            debug!(
                                "WebSocket handshake response FOUND: {}",
                                String::from_utf8_lossy(&response)
                            );
                            break;
                        } else {
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

        poll.registry().deregister(&mut stream)?;
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

    pub fn get_ref(&self) -> &TcpStream {
        self.ws.get_ref()
    }

    pub fn get_mut(&mut self) -> &mut TcpStream {
        self.ws.get_mut()
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

impl Read for WebSocketClient {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self.ws.read() {
            Ok(Message::Binary(data)) => {
                let len = data.len().min(buf.len());
                buf[..len].copy_from_slice(&data[..len]);
                Ok(len)
            }
            Ok(Message::Text(text)) => {
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

impl Write for WebSocketClient {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.flushed {
            let message = if buf.len() < 2 || buf.len() > (CHUNK_SIZE - 3) {
                debug!("Writing binary {} bytes", buf.len());
                tokio_tungstenite::tungstenite::Message::Binary(buf.to_vec())
            } else {
                tokio_tungstenite::tungstenite::Message::Text(
                    String::from_utf8_lossy(buf).to_string(),
                )
            };
            match self.ws.write(message) {
                Ok(_) =>  {
                    self.flushed = false;
                    return self.write(buf)
                },
                Err(e) => {
                    match e {
                        tungstenite::Error::Io(io_err)
                            if io_err.kind() == std::io::ErrorKind::WouldBlock =>
                        {
                            self.flushed = false;
                            Err(io::Error::new(io::ErrorKind::WouldBlock, "WouldBlock"))
                        }
                        _ =>  {
                            return Err(io::Error::new(io::ErrorKind::Other, e));
                        },
                    }
                }
            }
        }
        else {
            match self.ws.flush() {
                Ok(_) => {
                    self.flushed = true;

                    return Ok(buf.len());
                }
                Err(e) =>  {
                    match e {
                        tungstenite::Error::Io(io_err)
                            if io_err.kind() == std::io::ErrorKind::WouldBlock =>
                        {
                            // debug!("WouldBlock flush {}", io_err.to_string());
                           return  Err(io::Error::new(io::ErrorKind::WouldBlock, "WouldBlock"))
                        }
                        _ =>  {
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
                debug!("WebSocket flush success");
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
