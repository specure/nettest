use crate::handlers::BasicHandler;
use crate::state::TestPhase;
use anyhow::Result;
use bytes::BytesMut;
use log::debug;
use mio::{net::TcpStream, Interest, Poll, Token};
use std::io::{self, Read, Write};
use std::time::{Duration, Instant};
use std::sync::atomic::{AtomicU32, Ordering};

const MIN_PINGS: u32 = 10;
const MAX_PINGS: u32 = 200;
const PING_TEST_DURATION_NS: u64 = 1_000_000_000; // 1 second

pub struct PingHandler {
    token: Token,
    phase: TestPhase,
    pings_sent: AtomicU32,
    pings_received: AtomicU32,
    test_start_time: Option<Instant>,
    read_buffer: BytesMut,  // Buffer for reading responses
    write_buffer: BytesMut, // Buffer for writing requests
}

impl PingHandler {
    pub fn new(token: Token) -> Result<Self> {
        Ok(Self {
            token,
            phase: TestPhase::PingSendPing,
            pings_sent: AtomicU32::new(0),
            pings_received: AtomicU32::new(0),
            test_start_time: None,
            read_buffer: BytesMut::with_capacity(1024),  // Start with 1KB buffer
            write_buffer: BytesMut::with_capacity(1024), // Start with 1KB buffer
        })
    }

    pub fn get_ping_command(&self) -> String {
        "PING\n".to_string()
    }

    pub fn get_ok_command(&self) -> String {
        "OK\n".to_string()
    }
}

impl BasicHandler for PingHandler {
    fn on_read(&mut self, stream: &mut TcpStream, poll: &Poll) -> Result<()> {
        let mut buf = vec![0u8; 1024];
        match self.phase {
            TestPhase::PingReceivePong => {
                match stream.read(&mut buf) {
                    Ok(n) if n > 0 => {
                        self.read_buffer.extend_from_slice(&buf[..n]);
                        if let Some(pos) = self.read_buffer.windows(1).position(|w| w == b"\n") {
                            let line = String::from_utf8_lossy(&self.read_buffer[..pos]);
                            if line == "PONG" {
                                debug!("Received PONG");
                                self.pings_received.fetch_add(1, Ordering::SeqCst);
                                self.phase = TestPhase::PingSendOk;
                                poll.registry().reregister(
                                    stream,
                                    self.token,
                                    Interest::WRITABLE,
                                )?;
                            }
                            // Remove processed line from buffer
                            self.read_buffer = BytesMut::from(&self.read_buffer[pos + 1..]);
                        }
                    }
                    Ok(0) => {
                        debug!("Connection closed by peer");
                        return Err(
                            io::Error::new(io::ErrorKind::ConnectionAborted, "Connection closed").into(),
                        );
                    }
                    Ok(_) => {
                        debug!("Read 0 bytes");
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                        debug!("WouldBlock");
                        return Ok(());
                    }
                    Err(e) => return Err(e.into()),
                }
            }
            TestPhase::PingReceiveTime => {
                match stream.read(&mut buf) {
                    Ok(n) if n > 0 => {
                        self.read_buffer.extend_from_slice(&buf[..n]);
                        if let Some(pos) = self.read_buffer.windows(1).position(|w| w == b"\n") {
                            let line = String::from_utf8_lossy(&self.read_buffer[..pos]);
                            if line.starts_with("TIME") {
                                debug!("Received TIME response");
                                if let Some(time_ns) = line
                                    .split_whitespace()
                                    .nth(1)
                                    .and_then(|s| s.parse::<u64>().ok())
                                {
                                    debug!("Time: {} ns", time_ns);
                                    // Check if we need to send more pings
                                    if let Some(start_time) = self.test_start_time {
                                        let elapsed = start_time.elapsed();
                                        let pings_sent = self.pings_sent.load(Ordering::SeqCst);
                                        let pings_received = self.pings_received.load(Ordering::SeqCst);
                                        
                                        if elapsed.as_nanos() < PING_TEST_DURATION_NS as u128 {
                                            if pings_sent < MAX_PINGS {
                                                self.phase = TestPhase::PingSendPing;
                                                poll.registry().reregister(
                                                    stream,
                                                    self.token,
                                                    Interest::WRITABLE,
                                                )?;
                                            }
                                        } else if pings_received >= MIN_PINGS {
                                            // We've received enough pings and time is up
                                            self.phase = TestPhase::GetChunksReceiveAccept;
                                            poll.registry().reregister(
                                                stream,
                                                self.token,
                                                Interest::READABLE,
                                            )?;
                                        }
                                    }
                                }
                            }
                            // Remove processed line from buffer
                            self.read_buffer = BytesMut::from(&self.read_buffer[pos + 1..]);
                        }
                    }
                    Ok(0) => {
                        debug!("Connection closed by peer");
                        return Err(
                            io::Error::new(io::ErrorKind::ConnectionAborted, "Connection closed").into(),
                        );
                    }
                    Ok(_) => {
                        debug!("Read 0 bytes");
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                        debug!("WouldBlock");
                        return Ok(());
                    }
                    Err(e) => return Err(e.into()),
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn on_write(&mut self, stream: &mut TcpStream, poll: &Poll) -> Result<()> {
        match self.phase {
            TestPhase::PingSendPing => {
                if self.write_buffer.is_empty() {
                    debug!("Sending PING command");
                    self.write_buffer.extend_from_slice(self.get_ping_command().as_bytes());
                }

                match stream.write(&self.write_buffer) {
                    Ok(0) => {
                        debug!("Connection closed by peer");
                        return Err(io::Error::new(
                            io::ErrorKind::ConnectionAborted,
                            "Connection closed",
                        )
                        .into());
                    }
                    Ok(n) => {
                        self.write_buffer = BytesMut::from(&self.write_buffer[n..]);
                        if self.write_buffer.is_empty() {
                            debug!("Sent PING command");
                            self.pings_sent.fetch_add(1, Ordering::SeqCst);
                            if self.test_start_time.is_none() {
                                self.test_start_time = Some(Instant::now());
                            }
                            self.phase = TestPhase::PingReceivePong;
                            poll.registry().reregister(
                                stream,
                                self.token,
                                Interest::READABLE,
                            )?;
                        }
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                        debug!("WouldBlock");
                        return Ok(());
                    }
                    Err(e) => {
                        debug!("Error: {}", e);
                        return Err(e.into());
                    }
                }
            }
            TestPhase::PingSendOk => {
                if self.write_buffer.is_empty() {
                    debug!("Sending OK command");
                    self.write_buffer.extend_from_slice(self.get_ok_command().as_bytes());
                }

                match stream.write(&self.write_buffer) {
                    Ok(0) => {
                        debug!("Connection closed by peer");
                        return Err(io::Error::new(
                            io::ErrorKind::ConnectionAborted,
                            "Connection closed",
                        )
                        .into());
                    }
                    Ok(n) => {
                        self.write_buffer = BytesMut::from(&self.write_buffer[n..]);
                        if self.write_buffer.is_empty() {
                            debug!("Sent OK command");
                            self.phase = TestPhase::PingReceiveTime;
                            poll.registry().reregister(
                                stream,
                                self.token,
                                Interest::READABLE,
                            )?;
                        }
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                        debug!("WouldBlock");
                        return Ok(());
                    }
                    Err(e) => {
                        debug!("Error: {}", e);
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