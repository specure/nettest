use crate::handlers::BasicHandler;
use crate::state::TestPhase;
use anyhow::Result;
use log::debug;
use mio::{net::TcpStream, Interest, Poll, Token};
use std::io::{self, Read, Write};
use bytes::BytesMut;

pub struct GreetingHandler {
    token: Token,
    phase: TestPhase,
    token_sent: bool,
    read_buffer: BytesMut,  // Buffer for reading responses
    write_buffer: BytesMut, // Buffer for writing requests
}

impl GreetingHandler {
    pub fn new(token: Token) -> Result<Self> {
        Ok(Self {
            token,
            phase: TestPhase::GreetingSendConnectionType,
            token_sent: false,
            read_buffer: BytesMut::with_capacity(1024),  // Start with 1KB buffer
            write_buffer: BytesMut::with_capacity(1024), // Start with 1KB buffer
        })
    }

    pub fn get_token_command(&self) -> String {
        format!("TOKEN {}\n", self.token.0)
    }

    pub fn mark_token_sent(&mut self) {
        self.token_sent = true;
    }
}

impl BasicHandler for GreetingHandler {
    fn on_read(&mut self, stream: &mut TcpStream, poll: &Poll) -> Result<()> {
        let mut buf = vec![0u8; 1024];
        match self.phase {
            TestPhase::GreetingReceiveGreeting => {
                match stream.read(&mut buf) {
                    Ok(n) if n > 0 => {
                        self.read_buffer.extend_from_slice(&buf[..n]);
                        if let Some(pos) = self.read_buffer.windows(2).position(|w| w == b"\r\n") {
                            let line = String::from_utf8_lossy(&self.read_buffer[..pos]);
                            if line.contains("HTTP/1.1 101") {
                                debug!("[on_read] Received upgrade response");
                                
                                self.phase = TestPhase::GreetingSendToken;
                                poll.registry().reregister(
                                    stream,
                                    self.token,
                                    Interest::WRITABLE,
                                )?;
                            }
                            // Remove processed line from buffer
                            self.read_buffer = BytesMut::from(&self.read_buffer[pos + 2..]);
                        }
                    }
                    Ok(0) => {
                        debug!("[on_read] Connection closed by peer");
                        return Err(io::Error::new(
                            io::ErrorKind::ConnectionAborted,
                            "Connection closed",
                        )
                        .into());
                    }
                    Ok(_) => {
                        debug!("[on_read] Read 0 bytes");
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                        debug!("[on_read] WouldBlock");
                        return Ok(());
                    }
                    Err(e) => return Err(e.into()),
                }
            }
            TestPhase::GreetingReceiveVersion => {
                if let Some(pos) = self.read_buffer.windows(1).position(|w| w == b"\n") {
                    let line = String::from_utf8_lossy(&self.read_buffer[..pos]);
                    if line.starts_with("RMBTv") {
                        debug!("[on_read] Received version: {}", line);
                        self.read_buffer = BytesMut::from(&self.read_buffer[pos + 1..]); // Remove version including \n
                        self.phase = TestPhase::GreetingReceiveAcceptToken;
                        self.on_read(stream, poll)?;
                    }
                }
            }
            TestPhase::GreetingReceiveAcceptToken => {
                // Read until \n
                debug!("[on_read] Received accept token");
                if let Some(pos) = self.read_buffer.windows(1).position(|w| w == b"\n") {
                    let line = String::from_utf8_lossy(&self.read_buffer[..pos]);
                    if line.contains("ACCEPT TOKEN") {
                        debug!("[on_read] Received ACCEPT command");
                        self.read_buffer = BytesMut::from(&self.read_buffer[pos + 1..]); // Remove read portion including newline
                        self.phase = TestPhase::GreetingSendToken;
                        poll.registry().reregister(
                            stream,
                            self.token,
                            Interest::WRITABLE,
                        )?;
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn on_write(&mut self, stream: &mut TcpStream, poll: &Poll) -> Result<()> {
        match self.phase {
            TestPhase::GreetingSendConnectionType => {
                if self.write_buffer.is_empty() {
                    let upgrade_request = "GET /rmbt HTTP/1.1 \r\n\
                    Connection: Upgrade \r\n\
                    Upgrade: RMBT\r\n\
                    RMBT-Version: 1.2.0\r\n\
                    \r\n";
                    self.write_buffer.extend_from_slice(upgrade_request.as_bytes());
                }

                match stream.write(&self.write_buffer) {
                    Ok(0) => {
                        if stream.peer_addr().is_err() {
                            return Err(io::Error::new(
                                io::ErrorKind::ConnectionAborted,
                                "Connection closed",
                            )
                            .into());
                        }
                        return Ok(());
                    }
                    Ok(n) => {
                        self.write_buffer = BytesMut::from(&self.write_buffer[n..]);
                        if self.write_buffer.is_empty() {
                            poll.registry()
                                .reregister(stream, self.token, Interest::READABLE)?;
                            self.phase = TestPhase::GreetingReceiveGreeting;
                        }
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                        return Ok(());
                    }
                    Err(e) => {
                        return Err(e.into());
                    }
                }
            }
            TestPhase::GreetingSendToken => {
                if self.write_buffer.is_empty() {
                    debug!("[on_write] Sending token command");
                    self.write_buffer.extend_from_slice(self.get_token_command().as_bytes());
                }

                match stream.write(&self.write_buffer) {
                    Ok(0) => {
                        if stream.peer_addr().is_err() {
                            debug!("[on_write] Connection closed by peer");
                            return Err(io::Error::new(
                                io::ErrorKind::ConnectionAborted,
                                "Connection closed",
                            )
                            .into());
                        }
                        return Ok(());
                    }
                    Ok(n) => {
                        self.write_buffer = BytesMut::from(&self.write_buffer[n..]);
                        if self.write_buffer.is_empty() {
                            debug!("[on_write] Sent token command");
                            self.mark_token_sent();
                            debug!("[on_write] Registering for readable");
                            self.phase = TestPhase::GetChunksReceiveAccept;
                            poll.registry()
                                .reregister(stream, self.token, Interest::READABLE)?;
                        }
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                        debug!("[on_write] WouldBlock");
                        return Ok(());
                    }
                    Err(e) => {
                        debug!("[on_write] Error: {}", e);
                        return Err(e.into());
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn get_phase(&self) -> TestPhase {
        self.phase.clone()
    }
}
