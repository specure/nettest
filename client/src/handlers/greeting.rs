use crate::handlers::BasicHandler;
use crate::state::TestPhase;
use anyhow::Result;
use bytes::{Buf, BytesMut};
use log::debug;
use mio::{net::TcpStream, Interest, Poll, Token};
use std::io::{self, Read, Write};

pub struct GreetingHandler {
    token: Token,
    phase: TestPhase,
    token_sent: bool,
    buffer: Vec<u8>, // Buffer for accumulating data
}

impl GreetingHandler {
    pub fn new(token: Token) -> Result<Self> {
        Ok(Self {
            token,
            phase: TestPhase::GreetingSendConnectionType,
            token_sent: false,
            buffer: Vec::new(),
        })
    }

    pub fn get_token_command(&self) -> String {
        "TOKEN 1234567890abcdef\n".to_string()
    }

    pub fn mark_token_sent(&mut self) {
        self.token_sent = true;
    }
}

impl BasicHandler for GreetingHandler {
    fn on_read(&mut self, stream: &mut TcpStream, poll: &Poll) -> Result<()> {
        let mut buf = vec![0u8; 1024];
        match stream.read(&mut buf) {
            Ok(n) => {
                self.buffer.extend_from_slice(&buf[..n]);
                match self.phase {
                    TestPhase::GreetingReceiveGreeting => {
                        debug!("[on_read] Received greeting");
                        // Read until first \r\n\r\n
                        if let Some(pos) = self.buffer.windows(4).position(|w| w == b"\r\n\r\n") {
                            let line = String::from_utf8_lossy(&self.buffer[..pos]);
                            debug!("[on_read] HTTP Response: {}", line);
                            self.buffer = self.buffer[pos + 4..].to_vec(); // Remove greeting including \r\n\r\n
                            self.phase = TestPhase::GreetingReceiveVersion;
                            self.on_read(stream, poll)?;
                        }
                    }
                    TestPhase::GreetingReceiveVersion => {
                        if let Some(pos) = self.buffer.windows(1).position(|w| w == b"\n") {
                            let line = String::from_utf8_lossy(&self.buffer[..pos]);
                            if line.starts_with("RMBTv") {
                                debug!("[on_read] Received version: {}", line);
                                self.buffer = self.buffer[pos + 1..].to_vec(); // Remove version including \n
                                self.phase = TestPhase::GreetingReceiveAcceptToken;
                                debug!("[on_read] Registering for readable");
                                self.on_read(stream, poll)?;
                            }
                        }
                    }
                    TestPhase::GreetingReceiveAcceptToken => {
                        // Read until \n
                        debug!("[on_read] Received accept token");
                        if let Some(pos) = self.buffer.windows(1).position(|w| w == b"\n") {
                            let line = String::from_utf8_lossy(&self.buffer[..pos]);
                            if line.contains("ACCEPT") {
                                debug!("[on_read] Received ACCEPT command");
                                self.buffer = self.buffer[pos + 1..].to_vec(); // Remove read portion including newline
                                self.phase = TestPhase::GreetingSendToken;
                                poll.registry().reregister(
                                    stream,
                                    self.token,
                                    Interest::WRITABLE,
                                )?;
                            }
                        }
                    }
                    TestPhase::GreetingReceiveOK => {
                        // Read until \n
                        if let Some(pos) = self.buffer.windows(1).position(|w| w == b"\n") {
                            let line = String::from_utf8_lossy(&self.buffer[..pos]);
                            if line.contains("OK") {
                                debug!("[on_read] Received OK response");
                                self.buffer = self.buffer[pos + 1..].to_vec(); // Remove read portion including newline
                                self.phase = TestPhase::GetChunksSendChunksCommand;
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
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                debug!("[on_read] WouldBlock");
                return Ok(());
            }
            Err(e) => return Err(e.into()),
        }
        Ok(())
    }

    fn on_write(&mut self, stream: &mut TcpStream, poll: &Poll) -> Result<()> {
        if self.phase == TestPhase::GreetingSendConnectionType {
            let upgrade_request = "GET /rmbt HTTP/1.1 \r\n\
            Connection: Upgrade \r\n\
            Upgrade: RMBT\r\n\
            RMBT-Version: 1.2.0\r\n\
            \r\n";

            match stream.write(upgrade_request.as_bytes()) {
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
                Ok(_) => {
                    poll.registry()
                        .reregister(stream, self.token, Interest::READABLE)?;
                    debug!("[on_write] Sent upgrade request");
                    self.phase = TestPhase::GreetingReceiveGreeting;
                    debug!("[on_write] Registering for readable");
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
        } else if self.phase == TestPhase::GreetingSendToken {
            debug!("[on_write] Sending token command");
            match stream.write(self.get_token_command().as_bytes()) {
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
                Ok(_) => {
                    debug!("[on_write] Sent token command");
                    self.mark_token_sent();
                    debug!("[on_write] Registering for readable");
                    self.phase = TestPhase::GreetingReceiveOK;
                    poll.registry()
                        .reregister(stream, self.token, Interest::READABLE)?;
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
        Ok(())
    }

    fn get_phase(&self) -> TestPhase {
        self.phase.clone()
    }
}
