use anyhow::Result;
use log::{debug, trace};
use mio::{Interest, Poll, Token};
use std::io::{self, Read, Write};
use tungstenite::{Message, WebSocket};

use crate::{
    config::constants::CHUNK_SIZE, stream::rustls_server::RustlsServerStream, tokio_server::utils::websocket::{generate_handshake_response, Handshake}
};

#[derive(Debug)]
pub struct WebSocketRustlsServerStream {
    pub ws: WebSocket<RustlsServerStream>,
    pub flushed: bool,
    pub buffer: Vec<u8>,
}

impl WebSocketRustlsServerStream {
    pub fn from_rustls_server_stream(stream: RustlsServerStream) -> Result<Self> {
        // conn.set_buffer_limit(Some(1024 * 1024 * 10));

        let mut ws = WebSocket::from_raw_socket(stream, tungstenite::protocol::Role::Server, None);
        ws.set_config(|config| {
            config.max_message_size = Some(1024 * 1024 * 10);
            config.max_frame_size = Some(1024 * 1024 * 10);
            config.write_buffer_size = 1024 * 1024 * 10;
        });

        Ok(Self {
            ws,
            flushed: true,
            buffer: vec![],
        })
    }

    pub fn finish_server_handshake(&mut self, handshake: Handshake) -> Result<()> {
        let response = generate_handshake_response(&handshake).unwrap();
        // Write handshake response directly to the underlying stream
        let stream = self.ws.get_mut();
        stream.write_all(response.as_bytes())?;
        stream.flush()?;
        Ok(())
    }

    pub fn close(&mut self) -> Result<()> {
        self.ws
            .close(None)
            .map_err(|e| anyhow::anyhow!("WebSocket close error: {}", e))
    }

    pub fn get_mut(&mut self) -> &mut RustlsServerStream {
        self.ws.get_mut()
    }

    pub fn register(&mut self, poll: &Poll, token: Token, interest: Interest) -> io::Result<()> {
        self.ws.get_mut().register(poll, token, interest)
    }

    pub fn reregister(&mut self, poll: &Poll, token: Token, interest: Interest) -> io::Result<()> {
        self.ws.get_mut().reregister(poll, token, interest)
    }
}

impl Read for WebSocketRustlsServerStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut current_pos = 0;

        if !self.buffer.is_empty() {
            let take = self.buffer.len().min(buf.len());
            buf[..take].copy_from_slice(&self.buffer[..take]);
            self.buffer.drain(..take);
            current_pos = take;
            if current_pos == buf.len() {
                return Ok(current_pos);
            }
        }

        loop {
            match self.ws.read() {
                Ok(Message::Binary(data)) => {
                    let space = buf.len() - current_pos;
                    if data.len() <= space {
                        let len = data.len();
                        buf[current_pos..current_pos + len].copy_from_slice(&data[..len]);
                        current_pos += len;
                        if current_pos == buf.len() {
                            return Ok(current_pos);
                        }
                    } else {
                        buf[current_pos..].copy_from_slice(&data[..space]);
                        self.buffer.extend_from_slice(&data[space..]);
                        return Ok(buf.len());
                    }
                }
                Ok(Message::Text(text)) => {
                    let bytes = text.as_bytes();
                    let space = buf.len() - current_pos;
                    if bytes.len() <= space {
                        let len = bytes.len();
                        buf[current_pos..current_pos + len].copy_from_slice(&bytes[..len]);
                        trace!("Read {} bytes from WebSocket", len);
                        return Ok(current_pos + len);
                    } else {
                        buf[current_pos..].copy_from_slice(&bytes[..space]);
                        self.buffer.extend_from_slice(&bytes[space..]);
                        return Ok(buf.len());
                    }
                }
                Ok(Message::Close(_)) => return Ok(0),
                Ok(_) => return Ok(0),
                Err(e) => match e {
                    tungstenite::Error::Io(io_err)
                        if io_err.kind() == std::io::ErrorKind::WouldBlock =>
                    {
                        debug!("WouldBlock");
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
}

impl Write for WebSocketRustlsServerStream {
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
