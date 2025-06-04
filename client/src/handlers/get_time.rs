use crate::handlers::BasicHandler;
use crate::state::TestPhase;
use anyhow::Result;
use bytes::{Buf, BytesMut};
use log::{debug, info};
use mio::{net::TcpStream, Interest, Poll, Token};
use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

const TEST_DURATION_NS: u64 = 7_000_000_000; // 7 seconds
const MIN_CHUNK_SIZE: u32 = 4096; // 4KB
const MAX_CHUNK_SIZE: u32 = 4194304; // 4MB

pub struct GetTimeHandler {
    token: Token,
    phase: TestPhase,
    chunk_size: u32,
    test_start_time: Option<Instant>,
    bytes_received: u64,
    read_buffer: BytesMut,
    write_buffer: BytesMut,
    responses: Vec<(u64, u64)>, // (time_ns, bytes)
}

impl GetTimeHandler {
    pub fn new(token: Token) -> Result<Self> {
        Ok(Self {
            token,
            phase: TestPhase::GetTimeSendCommand,
            chunk_size: MIN_CHUNK_SIZE,
            test_start_time: None,
            bytes_received: 0,
            read_buffer: BytesMut::with_capacity(1024),
            write_buffer: BytesMut::with_capacity(1024),
            responses: Vec::new(),
        })
    }

    pub fn get_time_command(&self) -> String {
        format!("GETTIME {} {}\n", TEST_DURATION_NS / 1_000_000_000, self.chunk_size)
    }

    pub fn calculate_download_speed(&self, time_ns: u64) -> f64 {
        let speed = self.bytes_received as f64 / time_ns as f64;
        speed
    }

    pub fn get_ok_command(&self) -> String {
        "OK\n".to_string()
    }
}

impl BasicHandler for GetTimeHandler {
    fn on_read(&mut self, stream: &mut TcpStream, poll: &Poll) -> Result<()> {
        let mut buf = vec![0u8; self.chunk_size as usize];
        match self.phase {
            TestPhase::GetTimeReceiveAccept => {
                match stream.read(&mut buf) {
                    Ok(n) if n > 0 => {
                        self.read_buffer.extend_from_slice(&buf[..n]);
                        let line = String::from_utf8_lossy(&self.read_buffer);
                        debug!("Received line in Accept phase: {}", line);

                        if line.contains("ACCEPT GETCHUNKS GETTIME PUT PUTNORESULT PING QUIT") {
                            debug!("Received correct ACCEPT line");
                            self.phase = TestPhase::GetTimeSendCommand;
                            self.read_buffer.clear();
                            poll.registry().reregister(
                                stream,
                                self.token,
                                Interest::WRITABLE,
                            )?;
                        }
                        self.read_buffer.clear();
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
                        debug!("Would block in GetTimeReceiveAccept");
                        return Ok(());
                    }
                    Err(e) => return Err(e.into()),
                }
            }
            TestPhase::GetTimeReceiveChunk => {
                match stream.read(&mut buf) {
                    Ok(n) if n > 0 => {
                        self.bytes_received += n as u64;
                        // Check if this is the last chunk (has termination byte 0xFF)
                        let is_last_chunk = buf[n - 1] == 0xFF;
                        if is_last_chunk {
                            debug!("Received last chunk");
                            self.phase = TestPhase::GetTimeSendOk;
                            poll.registry().reregister(
                                stream,
                                self.token,
                                Interest::WRITABLE,
                            )?;
                        } else {
                            // Continue reading
                            poll.registry().reregister(
                                stream,
                                self.token,
                                Interest::READABLE,
                            )?;
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
                        debug!("Would block in GetTimeReceiveChunk");
                        return Ok(());
                    }
                    Err(e) => return Err(e.into()),
                }
            }
            TestPhase::GetTimeReceiveTime => {
                match stream.read(&mut buf) {
                    Ok(n) if n > 0 => {
                        self.read_buffer.extend_from_slice(&buf[..n]);
                        let line = String::from_utf8_lossy(&self.read_buffer);
                        debug!("Received line in GetTimeReceiveTime: {}", line);

                        if line.contains("ACCEPT GETCHUNKS GETTIME PUT PUTNORESULT PING QUIT\n") {
                            debug!("Received TIME response");
                            if let Some(time_ns) = line
                                .split_whitespace()
                                .nth(1)
                                .and_then(|s| s.parse::<u64>().ok())
                            {
                                info!("Time download: {} ns", time_ns);
                                info!("Download speed: {} MB/s  {} GB/s", self.calculate_download_speed(time_ns), self.calculate_download_speed(time_ns) / 1000.0);

                                self.responses.push((time_ns, self.bytes_received));
                                self.phase = TestPhase::PutNoResultSendCommand;
                                poll.registry().reregister(
                                    stream,
                                    self.token,
                                    Interest::WRITABLE,
                                )?;
                            }
                            self.read_buffer.clear();
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
                        debug!("Would block in GetTimeReceiveTime");
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
        debug!("Writing to stream in GetTimeSendCommand {:?}", self.phase);
        match self.phase {
            TestPhase::GetTimeSendCommand => {
                if self.write_buffer.is_empty() {
                    self.write_buffer
                        .extend_from_slice(self.get_time_command().as_bytes());
                }
                debug!("Sending command: {}", self.get_time_command());
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
                            self.phase = TestPhase::GetTimeReceiveChunk;
                            self.test_start_time = Some(Instant::now());
                            poll.registry()
                                .reregister(stream, self.token, Interest::READABLE)?;
                        }
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                        return Ok(());
                    }
                    Err(e) => return Err(e.into()),
                }
            }
            TestPhase::GetTimeSendOk => {
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
                        self.write_buffer = BytesMut::from(&self.write_buffer[n..]);
                        if self.write_buffer.is_empty() {
                            self.phase = TestPhase::GetTimeReceiveTime;
                            poll.registry()
                                .reregister(stream, self.token, Interest::READABLE)?;
                        }
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                        return Ok(());
                    }
                    Err(e) => return Err(e.into()),
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