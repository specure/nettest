use crate::handlers::BasicHandler;
use crate::state::TestPhase;
use anyhow::Result;
use bytes::{Buf, BytesMut};
use fastrand;
use log::{debug, info, trace};
use mio::{net::TcpStream, Interest, Poll, Token};
use std::io::{self, IoSlice, Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

const TEST_DURATION_NS: u64 = 7_000_000_000; // 7 seconds
const MIN_CHUNK_SIZE: u32 = 4096; // 4KB
const MAX_CHUNK_SIZE: u64 = 4194304; // 4MB
const BUFFER_SIZE: usize = 4 * 1024 * 1024; // 4mb
const TERMINATION_BYTE_LAST: [u8; 1] = [0xFF];
const TERMINATION_BYTE_NORMAL: [u8; 1] = [0x00];

pub struct PutHandler {
    token: Token,
    phase: TestPhase,
    chunk_size: u64,
    test_start_time: Option<Instant>,
    bytes_sent: u64,
    write_buffer: BytesMut,
    read_buffer: BytesMut,
    data_buffer: Vec<u8>,
    data_termination_buffer: Vec<u8>,
    responses: Vec<(u64, u64)>, // (time_ns, bytes)
}

impl PutHandler {
    pub fn new(token: Token) -> Result<Self> {
        // Create and fill buffer with random data
        let mut data_buffer = vec![0u8; BUFFER_SIZE];
        for byte in &mut data_buffer {
            *byte = fastrand::u8(..);
        }
        data_buffer[BUFFER_SIZE - 1] = 0x00;
        let mut data_termination_buffer = vec![0u8; BUFFER_SIZE];
        for byte in &mut data_termination_buffer {
            *byte = fastrand::u8(..);
        }
        data_termination_buffer[BUFFER_SIZE - 1] = 0xFF;

        Ok(Self {
            token,
            phase: TestPhase::PutSendCommand,
            chunk_size: MAX_CHUNK_SIZE,
            test_start_time: None,
            bytes_sent: 0,
            write_buffer: BytesMut::with_capacity(1024),
            read_buffer: BytesMut::with_capacity(1024),
            data_buffer,
            data_termination_buffer,
            responses: Vec::new(),
        })
    }

    pub fn get_put_command(&self) -> String {
        format!("PUT {}\n", self.chunk_size)
    }
}

impl BasicHandler for PutHandler {
    fn on_read(&mut self, stream: &mut TcpStream, poll: &Poll) -> Result<()> {
        let mut buf = vec![0u8; 64];
        match self.phase {
            TestPhase::PutReceiveAccept => {
                match stream.read(&mut buf) {
                    Ok(n) => {
                        if n > 0 {
                            self.read_buffer.extend_from_slice(&buf[..n]);
                            let line = String::from_utf8_lossy(&self.read_buffer);
                            debug!("Received line in Accept phase: {}", line);

                            if line.contains("ACCEPT GETCHUNKS GETTIME PUT PUTNORESULT PING QUIT") {
                                debug!("Received correct ACCEPT line");
                                self.phase = TestPhase::PutSendCommand;
                                self.read_buffer.clear();
                                poll.registry().reregister(
                                    stream,
                                    self.token,
                                    Interest::WRITABLE,
                                )?;
                            }
                            self.read_buffer.clear();
                        } else {
                            debug!("Connection closed by peer in Accept phase");
                            return Err(io::Error::new(
                                io::ErrorKind::ConnectionAborted,
                                "Connection closed",
                            )
                            .into());
                        }
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                        debug!("Would block in PutReceiveAccept");
                        return Ok(());
                    }
                    Err(e) => return Err(e.into()),
                }
            }
            TestPhase::PutReceiveOk => {
                match stream.read(&mut buf) {
                    Ok(n) => {
                        if n > 0 {
                            self.read_buffer.extend_from_slice(&buf[..n]);
                            let line = String::from_utf8_lossy(&self.read_buffer);
                            debug!("Received line in PutReceiveOk: {}", line);

                            if line.trim() == "OK" {
                                self.phase = TestPhase::PutSendChunks;
                                poll.registry().reregister(
                                    stream,
                                    self.token,
                                    Interest::WRITABLE | Interest::READABLE,
                                )?;
                            }
                            self.read_buffer.clear();
                        }
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                        debug!("Would block in PutReceiveOk");
                        return Ok(());
                    }
                    Err(e) => return Err(e.into()),
                }
            }
            TestPhase::PutSendChunks => {
                match stream.read(&mut buf) {
                    Ok(n) => {
                        if n > 0 {
                            self.read_buffer.extend_from_slice(&buf[..n]);
                            let line = String::from_utf8_lossy(&self.read_buffer);
                            debug!("Received TIME  bytes response {}", line);

                            if line.ends_with("\n") {
                                if let Some(time_ns) = line
                                    .split_whitespace()
                                    .nth(1)
                                    .and_then(|s| s.parse::<u64>().ok())
                                {
                                    if let Some(bytes) = line
                                        .split_whitespace()
                                        .nth(3)
                                        .and_then(|s| s.parse::<u64>().ok())
                                    {
                                        debug!("Time: {} ns, Bytes: {}", time_ns, bytes);
                                        self.responses.push((time_ns, bytes));
                                        // self.phase = TestPhase::PutSendChunks;
                                        self.read_buffer.clear();
                                        // return Ok(());
                                        poll.registry().reregister(
                                            stream,
                                            self.token,
                                            Interest::WRITABLE ,
                                        )?;
                                    }
                                }
                                return Ok(());
                            }
                        } else {
                            debug!("Connection closed by peer");
                            return Err(io::Error::new(
                                io::ErrorKind::ConnectionAborted,
                                "Connection closed",
                            )
                            .into());
                        }
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                        debug!("Would block in PutReceiveTime");
                        return Ok(());
                    }
                    Err(e) => return Err(e.into()),
                }
            }
            TestPhase::PutReceiveTime => {
                match stream.read(&mut buf) {
                    Ok(n) => {
                        if n > 0 {
                            self.read_buffer.extend_from_slice(&buf[..n]);
                            let line = String::from_utf8_lossy(&self.read_buffer);
                            debug!("Received line in PutReceiveTime: {}", line);

                            if line.starts_with("TIME") {
                                debug!("Received TIME response");
                                if let Some(time_ns) = line
                                    .split_whitespace()
                                    .nth(1)
                                    .and_then(|s| s.parse::<u64>().ok())
                                {
                                    debug!("Final Time: {} ns", time_ns);
                                    self.phase = TestPhase::End;
                                    poll.registry().reregister(
                                        stream,
                                        self.token,
                                        Interest::WRITABLE,
                                    )?;
                                }
                                self.read_buffer.clear();
                            }
                        } else {
                            debug!("Connection closed by peer");
                            return Err(io::Error::new(
                                io::ErrorKind::ConnectionAborted,
                                "Connection closed",
                            )
                            .into());
                        }
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                        debug!("Would block in PutReceiveTime");
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
            TestPhase::PutSendCommand => {
                if self.write_buffer.is_empty() {
                    self.write_buffer
                        .extend_from_slice(self.get_put_command().as_bytes());
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
                            self.phase = TestPhase::PutReceiveOk;
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
            TestPhase::PutSendChunks => {
                if self.test_start_time.is_none() {
                    self.test_start_time = Some(Instant::now());
                }

                if let Some(start_time) = self.test_start_time {
                    let elapsed = start_time.elapsed();
                    let mut is_last = elapsed.as_nanos() >= TEST_DURATION_NS as u128;

                    // Get current position in buffer
                    let current_pos = self.bytes_sent & (BUFFER_SIZE as u64 - 1);

                    // Choose buffer based on whether time is up
                    let buffer = if is_last {
                        &self.data_termination_buffer
                    } else {
                        &self.data_buffer
                    };

                    let mut remaining = &buffer[current_pos as usize..];
                    while !is_last {
                        if remaining.is_empty() {
                            is_last = elapsed.as_nanos() >= TEST_DURATION_NS as u128;
                            if is_last {
                                return Ok(());
                            } else {
                                remaining = &self.data_buffer;
                            }
                        }

                        // Write from current position
                        match stream.write(remaining) {
                            Ok(written) => {
                                self.bytes_sent += written as u64;
                                remaining = &remaining[written..];
                                if written > 0 {
                                    return Ok(());
                                }
                            }
                            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                                return Ok(());
                            }
                            Err(e) => return Err(e.into()),
                        }
                    }

                    while is_last && !remaining.is_empty() {
                        match stream.write(remaining) {
                            Ok(written) => {
                                self.bytes_sent += written as u64;
                                remaining = &remaining[written..];
                            }
                            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                                debug!("Write would block, reregistering for write last chunk");
                                return Ok(());
                            }
                            Err(e) => return Err(e.into()),
                        }
                    }

                    debug!("PutSendChunks finished with last chunk");
                    self.phase = TestPhase::PutReceiveTime;
                    poll.registry()
                        .reregister(stream, self.token, Interest::READABLE)?;
                }
            }
            _ => {
                debug!("PutHandler phase {:?}", self.phase);
            }
        }
        Ok(())
    }

    fn get_phase(&self) -> TestPhase {
        self.phase.clone()
    }
}