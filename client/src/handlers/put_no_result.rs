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

const TEST_DURATION_NS: u64 = 7_000_000_000; // 3 seconds
const MIN_CHUNK_SIZE: u32 = 4096; // 4KB
const MAX_CHUNK_SIZE: u64 = 4194304; // 2MB
const BUFFER_SIZE: usize = 4 * 1024 * 1024; // 4mb
const TERMINATION_BYTE_LAST: [u8; 1] = [0xFF];
const TERMINATION_BYTE_NORMAL: [u8; 1] = [0x00];

pub struct PutNoResultHandler {
    token: Token,
    phase: TestPhase,
    chunk_size: u64,
    test_start_time: Option<Instant>,
    bytes_sent: u64,
    write_buffer: BytesMut,
    read_buffer: BytesMut,
    data_buffer: Vec<u8>,
    data_termination_buffer: Vec<u8>,
    upload_speed: Option<f64>,
}

impl PutNoResultHandler {
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
            phase: TestPhase::PutNoResultSendCommand,
            chunk_size: MAX_CHUNK_SIZE,
            test_start_time: None,
            bytes_sent: 0,
            write_buffer: BytesMut::with_capacity(1024),
            read_buffer: BytesMut::with_capacity(1024),
            data_buffer,
            data_termination_buffer,
            upload_speed: None,
        })
    }

    pub fn get_put_no_result_command(&self) -> String {
        format!("PUTNORESULT {}\n", self.chunk_size)
    }

    fn calculate_upload_speed(&mut self, time_ns: u64) {
        let bytes = self.bytes_sent;
        // Convert nanoseconds to seconds, ensuring we don't lose precision
        let time_seconds = if time_ns > u64::MAX / 1_000_000_000 {
            // If time_ns is very large, divide first to avoid overflow
            (time_ns / 1_000_000_000) as f64 + (time_ns % 1_000_000_000) as f64 / 1_000_000_000.0
        } else {
            time_ns as f64 / 1_000_000_000.0
        };
        let speed_bps: f64 = bytes as f64 / time_seconds; // Convert to bytes per second
        self.upload_speed = Some(speed_bps);
        info!("Upload speed calculation:");
        info!("  Total bytes sent: {}", bytes);
        info!(
            " Total bytes sent GB: {}",
            bytes as f64 / (1024.0 * 1024.0 * 1024.0)
        );
        info!("  Total time: {:.9} seconds ({} ns)", time_seconds, time_ns);
        info!(
            "  Speed: {:.2} MB/s ({:.2} GB/s)  GBit/s: {}  Mbit/s: {}",
            speed_bps / (1024.0 * 1024.0),
            speed_bps / (1024.0 * 1024.0 * 1024.0),
            speed_bps * 8.0 / (1024.0 * 1024.0 * 1024.0),
            speed_bps * 8.0 / (1024.0 * 1024.0)
        );
    }
}

impl BasicHandler for PutNoResultHandler {
    fn on_read(&mut self, stream: &mut TcpStream, poll: &Poll) -> Result<()> {
        let mut buf = vec![0u8; 64];
        match self.phase {
            TestPhase::PutNoResultReceiveAccept => {
                match stream.read(&mut buf) {
                    Ok(n) => {
                        if n > 0 {
                            self.read_buffer.extend_from_slice(&buf[..n]);
                            let line = String::from_utf8_lossy(&self.read_buffer);
                            debug!("Received line in Accept phase: {}", line);

                            if line.contains("ACCEPT GETCHUNKS GETTIME PUT PUTNORESULT PING QUIT") {
                                debug!("Received correct ACCEPT line");
                                self.phase = TestPhase::PutNoResultSendCommand;
                                // Clear the buffer completely for new test
                                self.read_buffer.clear();
                                poll.registry().reregister(
                                    stream,
                                    self.token,
                                    Interest::WRITABLE,
                                )?;
                            } else {
                                debug!("Received unexpected line in Accept phase: {}", line);
                            }
                            debug!(
                                "Read {} bytes in Accept phase, total buffer size: {}",
                                n,
                                self.read_buffer.len()
                            );
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
                        debug!("Would block in PutNoResultReceiveAccept, checking buffer");
                        // Now we know we have all data, check for complete line
                        return Ok(());
                    }
                    Err(e) => {
                        debug!("Error reading in Accept phase: {}", e);
                        return Err(e.into());
                    }
                }
            }
            TestPhase::PutNoResultReceiveOk => {
                match stream.read(&mut buf) {
                    Ok(n) => {
                        if n > 0 {
                            self.read_buffer.extend_from_slice(&buf[..n]);

                            let line = String::from_utf8_lossy(&self.read_buffer);
                            debug!("Received line  PutNoResultReceiveOk: {}", line);

                            if line.trim() == "OK" {
                                self.phase = TestPhase::PutNoResultSendChunks;
                                poll.registry().reregister(
                                    stream,
                                    self.token,
                                    Interest::WRITABLE,
                                )?;
                            }
                            // Remove processed line from buffer
                            self.read_buffer.clear();
                        }
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                        debug!("Would block in PutNoResultReceiveOk");
                        return Ok(());
                    }
                    Err(e) => return Err(e.into()),
                }
            }
            TestPhase::PutNoResultReceiveTime => {
                match stream.read(&mut buf) {
                    Ok(n) => {
                        if n > 0 {
                            self.read_buffer.extend_from_slice(&buf[..n]);
                            debug!(
                                "Read {} bytes, total buffer size: {}",
                                n,
                                self.read_buffer.len()
                            );

                            // Check if we have a complete line
                            let line = String::from_utf8_lossy(&self.read_buffer);
                            debug!("Received line PutNoResultReceiveTime: {}", line);

                            if line.contains("ACCEPT GETCHUNKS GETTIME PUT PUTNORESULT PING QUIT\n") {
                                debug!("Received ACCEPT message");
                                // Now parse TIME from the buffer
                                if let Some(time_line) = line.lines().find(|l| l.starts_with("TIME")) {
                                    if let Some(time_ns) = time_line
                                        .split_whitespace()
                                        .nth(1)
                                        .and_then(|s| s.parse::<u64>().ok())
                                    {
                                        debug!("Time: {} ns", time_ns);
                                        self.calculate_upload_speed(time_ns);
                                    }
                                }
                                self.phase = TestPhase::PutSendCommand;
                                poll.registry().reregister(
                                    stream,
                                    self.token,
                                    Interest::WRITABLE,
                                )?;
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
                        debug!("Would block in PutNoResultReceiveTime");
                        return Ok(());
                    }
                    Err(e) => {
                        debug!("Error in PutNoResultReceiveTime: {}", e);
                        return Err(e.into());
                    }
                }
            }
            _ => {
                debug!("Unknown phase11: {:?}", self.phase);
                return Ok(());
            }
        }
        Ok(())
    }

    fn on_write(&mut self, stream: &mut TcpStream, poll: &Poll) -> Result<()> {
        match self.phase {
            TestPhase::PutNoResultSendCommand => {
                if self.write_buffer.is_empty() {
                    self.write_buffer
                        .extend_from_slice(self.get_put_no_result_command().as_bytes());
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
                            self.phase = TestPhase::PutNoResultReceiveOk;
                            poll.registry()
                                .reregister(stream, self.token, Interest::READABLE)?;
                            return Ok(());
                        } else {
                            return Ok(());
                        }
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                        return Ok(());
                    }
                    Err(e) => return Err(e.into()),
                }
            }
            TestPhase::PutNoResultSendChunks => {
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
                            // debug!("PutNoResultSendChunks processing chunk");
                            // Add new chunk
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
                            }
                            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                                // debug!("PutNoResultSendChunks processing chunk");
                                return Ok(());
                            }
                            Err(e) => {
                                return Err(e.into());
                            }
                        }
                    }

                    while is_last && !remaining.is_empty() {
                        // Send last chunk
                        match stream.write(remaining) {
                            Ok(written) => {
                                self.bytes_sent += written as u64;
                                remaining = &remaining[written..];
                            }
                            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                                debug!("Write would block, reregistering for write last chunk");
                                return Ok(());
                            }
                Err(e) => {
                                debug!("Error in PutNoResultSendChunks: {}", e);
                                return Err(e.into());
                            }
                        }
                    }
                    debug!("PutNoResultSendChunks finished with kast chunk");
                    self.phase = TestPhase::PutNoResultReceiveTime;
                    poll.registry()
                        .reregister(stream, self.token, Interest::READABLE)?;
                    return Ok(());
                } else {
                    debug!("No start time set");
                    return Ok(());
                }
            }
            _ => {
                debug!("Unknown phase11: {:?}", self.phase);
                return Ok(());
            }
        }
    }

    fn get_phase(&self) -> TestPhase {
        self.phase.clone()
    }
}

