use crate::handlers::BasicHandler;
use crate::state::TestPhase;
use anyhow::Result;
use bytes::{Buf, BytesMut};
use log::debug;
use mio::{net::TcpStream, Interest, Poll, Token};
use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};
use std::sync::Mutex;

const MIN_PINGS: u32 = 10;
const MAX_PINGS: u32 = 200;
const PING_DURATION_NS: u64 = 1_000_000_000; // 1 second
const PONG_RESPONSE: &[u8] = b"PONG\n";

pub struct PingHandler {
    token: Token,
    phase: TestPhase,
    pings_sent: AtomicU32,
    pings_received: AtomicU32,
    test_start_time: Option<Instant>,
    unprocessed_bytes: AtomicU32,
    write_buffer: BytesMut,
    time_result: Option<u64>,
    ping_times: Mutex<Vec<u64>>, // Store all ping times for median calculation
    time_buffer: BytesMut, // Buffer for TIME response
}

impl PingHandler {
    pub fn new(token: Token) -> Result<Self> {
        Ok(Self {
            token,
            phase: TestPhase::PingSendPing,
            pings_sent: AtomicU32::new(0),
            pings_received: AtomicU32::new(0),
            test_start_time: None,
            unprocessed_bytes: AtomicU32::new(0),
            write_buffer: BytesMut::with_capacity(1024),
            time_result: None,
            ping_times: Mutex::new(Vec::with_capacity(MAX_PINGS as usize)),
            time_buffer: BytesMut::with_capacity(128), // "TIME " + u64 max + "\n" = 5 + 20 + 1 = 26 bytes
        })
    }

    pub fn get_ping_command(&self) -> String {
        "PING\n".to_string()
    }

    pub fn get_ok_command(&self) -> String {
        "OK\n".to_string()
    }

    pub fn get_median_latency(&self) -> Option<u64> {
        let times = self.ping_times.lock().unwrap();
        if times.is_empty() {
            return None;
        }
        
        let mut sorted_times = times.clone();
        sorted_times.sort_unstable();
        
        let mid = sorted_times.len() / 2;
        if sorted_times.len() % 2 == 0 {
            Some((sorted_times[mid - 1] + sorted_times[mid]) / 2)
        } else {
            Some(sorted_times[mid])
        }
    }
}

impl BasicHandler for PingHandler {
    fn on_read(&mut self, stream: &mut TcpStream, poll: &Poll) -> Result<()> {
        let mut buf = vec![0u8; 1024];

        match self.phase {
            TestPhase::PingReceivePong => {
                let mut buf1 = vec![0u8; PONG_RESPONSE.len()];
                match stream.read(&mut buf1) {
                    Ok(n) if n > 0 => {
                        debug!("Received PONG {}", String::from_utf8_lossy(&buf1));
                            self.phase = TestPhase::PingSendOk;
                            poll.registry().reregister(
                                stream,
                                self.token,
                                Interest::WRITABLE,
                            )?;
                    }
                    Ok(0) => {
                        debug!("Connection closed by peer");
                        return Err(io::Error::new(
                            io::ErrorKind::ConnectionAborted,
                            "Connection closed",
                        )
                        .into());
                    }
                    Ok(_) => {
                        debug!("Read 0 bytes");
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                        debug!("WouldBlock PingReceivePong");
                        return Ok(());
                    }
                    Err(e) => return Err(e.into()),
                }
            }
            TestPhase::PingReceiveTime => {
                let mut buf = vec![0u8; 128];
                match stream.read(&mut buf) {
                    Ok(n) if n > 0 => {
                        self.time_buffer.extend_from_slice(&buf[..n]);
                        let buffer_str = String::from_utf8_lossy(&self.time_buffer);
                        debug!("Received data: {}", buffer_str);

                        if buffer_str.contains("ACCEPT GETCHUNKS GETTIME PUT PUTNORESULT PING QUIT\n") {
                            self.pings_received.fetch_add(1, Ordering::SeqCst);
                            // Parse time from the message
                            if let Some(time_start) = buffer_str.find("TIME ") {
                                if let Some(time_end) = buffer_str[time_start..].find('\n') {
                                    let time_str = &buffer_str[time_start + 5..time_start + time_end];
                                    if let Ok(time_ns) = time_str.parse::<u64>() {
                                        debug!("Time: {} ns", time_ns);
                                        // Store the time for median calculation
                                        if let Ok(mut times) = self.ping_times.lock() {
                                            times.push(time_ns);
                                        }
                                        
                                        if let Some(start_time) = self.test_start_time {
                                            let elapsed = start_time.elapsed();
                                            let pings_sent = self.pings_sent.load(Ordering::SeqCst);
                                            let pings_received = self.pings_received.load(Ordering::SeqCst);

                                            if elapsed.as_nanos() < PING_DURATION_NS as u128
                                                && pings_sent < MAX_PINGS
                                                && pings_received >= pings_sent
                                            {
                                                debug!("Finishing Pings and sending PING");
                                                self.phase = TestPhase::PingSendPing;
                                                poll.registry().reregister(
                                                    stream,
                                                    self.token,
                                                    Interest::WRITABLE,
                                                )?;
                                            } else {
                                                debug!("Finishing Pings and sending GETTIME");
                                                // Calculate final median latency
                                                if let Some(median) = self.get_median_latency() {
                                                    debug!("Final median latency: {} ns", median);
                                                    self.time_result = Some(median);
                                                }
                                                self.phase = TestPhase::GetTimeSendCommand;
                                                poll.registry().reregister(
                                                    stream,
                                                    self.token,
                                                    Interest::WRITABLE,
                                                )?;
                                            }
                                        }
                                    }
                                }
                            }
                            self.time_buffer.clear();
                        }
                    }
                    Ok(0) => {
                        debug!("Connection closed by peer");
                        return Err(io::Error::new(
                            io::ErrorKind::ConnectionAborted,
                            "Connection closed",
                        )
                        .into());
                    }
                    Ok(_) => {
                        debug!("Read 0 bytes");
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                        debug!("WouldBlock PingReceiveTime");

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
                    self.write_buffer
                        .extend_from_slice(self.get_ping_command().as_bytes());
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
                        debug!("Ping sent");
                        self.pings_sent.fetch_add(1, Ordering::SeqCst);
                        if self.test_start_time.is_none() {
                            self.test_start_time = Some(Instant::now());
                        }
                        self.phase = TestPhase::PingReceivePong;
                        self.write_buffer = BytesMut::from(&self.write_buffer[n..]);
                        if self.write_buffer.is_empty() {
                            poll.registry()
                                .reregister(stream, self.token, Interest::READABLE)?;
                        }
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                        debug!("WouldBlock PingSendPing");
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
                    self.write_buffer
                        .extend_from_slice(self.get_ok_command().as_bytes());
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
                        debug!("Ok sent");
                        self.phase = TestPhase::PingReceiveTime;
                        self.write_buffer = BytesMut::from(&self.write_buffer[n..]);
                        if self.write_buffer.is_empty() {
                            poll.registry()
                                .reregister(stream, self.token, Interest::READABLE)?;
                        }
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                        debug!("WouldBlock PingSendOk");
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