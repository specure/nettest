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
const BUFFER_SIZE: usize = 10 * 1024 * 1024; // 100MB
const TERMINATION_BYTE_LAST: [u8; 1] = [0xFF];
const TERMINATION_BYTE_NORMAL: [u8; 1] = [0x00];

pub struct PutNoResultHandler {
    token: Token,
    phase: TestPhase,
    chunk_size: u64,
    test_start_time: Option<Instant>,
    bytes_sent: AtomicU64,
    write_buffer: BytesMut,
    read_buffer: BytesMut,
    data_buffer: Vec<u8>,
    upload_speed: Option<f64>,
}

impl PutNoResultHandler {
    pub fn new(token: Token) -> Result<Self> {
        // Create and fill buffer with random data
        let mut data_buffer = vec![0u8; BUFFER_SIZE];
        for byte in &mut data_buffer {
            *byte = fastrand::u8(..);
        }

        Ok(Self {
            token,
            phase: TestPhase::PutNoResultSendCommand,
            chunk_size: MAX_CHUNK_SIZE,
            test_start_time: None,
            bytes_sent: AtomicU64::new(0),
            write_buffer: BytesMut::with_capacity(1024),
            read_buffer: BytesMut::with_capacity(1024),
            data_buffer,
            upload_speed: None,
        })
    }

    pub fn get_put_no_result_command(&self) -> String {
        format!("PUTNORESULT {}\n", self.chunk_size)
    }

    fn calculate_upload_speed(&mut self, time_ns: u64) {
        let bytes = self.bytes_sent.load(Ordering::SeqCst) ;

        
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
        info!("  Total time: {:.9} seconds ({} ns)", time_seconds, time_ns);
        info!("  Speed: {:.2} MB/s ({:.2} GB/s)", speed_bps / (1024.0 * 1024.0), speed_bps / (1024.0 * 1024.0 * 1024.0));
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
                        if let Some(pos) = self.read_buffer.windows(1).position(|w| w == b"\n") {
                            let line = String::from_utf8_lossy(&self.read_buffer[..pos]);
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
                        }
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
                            debug!(
                                "Read {} bytes, total buffer size: {}",
                                n,
                                self.read_buffer.len()
                            );

                            // Check if we have a complete line
                            if let Some(pos) = self.read_buffer.windows(1).position(|w| w == b"\n")
                            {
                                let line = String::from_utf8_lossy(&self.read_buffer[..pos]);
                                debug!("Received line: {}", line);

                                if line.trim() == "OK" {
                                    debug!("Received OK for PUTNORESULT");
                                    self.phase = TestPhase::PutNoResultSendChunks;
                                    poll.registry().reregister(
                                        stream,
                                        self.token,
                                        Interest::WRITABLE,
                                    )?;
                                }
                                // Remove processed line from buffer
                                self.read_buffer = BytesMut::from(&self.read_buffer[pos + 1..]);
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
                            if let Some(pos) = self.read_buffer.windows(1).position(|w| w == b"\n")
                            {
                                let line = String::from_utf8_lossy(&self.read_buffer[..pos]);
                                debug!("Received line: {}", line);

                                if line.starts_with("TIME") {
                                    debug!("Received final TIME response");
                                    if let Some(time_ns) = line
                                        .split_whitespace()
                                        .nth(1)
                                        .and_then(|s| s.parse::<u64>().ok())
                                    {
                                        debug!("Final time: {} ns", time_ns);
                                        self.calculate_upload_speed(time_ns);
                                        self.phase = TestPhase::End;
                                        poll.registry().reregister(
                                            stream,
                                            self.token,
                                            Interest::WRITABLE,
                                        )?;
                                    }
                                }
                                // Remove processed line from buffer
                                self.read_buffer = BytesMut::from(&self.read_buffer[pos + 1..]);
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
                    let is_last = elapsed.as_nanos() >= TEST_DURATION_NS as u128;
                    let mut slices = Vec::new();

                    // Normal chunk sending
                    let current_pos =
                        self.bytes_sent.load(Ordering::SeqCst) & (self.chunk_size - 1) as u64;
                    let remaining_in_chunk: u64 = self.chunk_size - current_pos;

                    // Calculate how many bytes we can send in this iteration
                    let bytes_to_send: u64 = if remaining_in_chunk == 0 {
                        self.chunk_size
                    } else {
                        remaining_in_chunk
                    };

                    let chunk = &self.data_buffer[0..(bytes_to_send - 1) as usize];
                    slices.push(IoSlice::new(chunk));
                    if is_last {
                        slices.push(IoSlice::new(&TERMINATION_BYTE_LAST));
                    } else {
                        slices.push(IoSlice::new(&TERMINATION_BYTE_NORMAL));
                    }

                    

                    match stream.write_vectored(&slices) {
                        Ok(n) => {
                            // Update bytes sent counter
                            self.bytes_sent.fetch_add(n as u64, Ordering::SeqCst);

                            if is_last && self.bytes_sent.load(Ordering::SeqCst) & (self.chunk_size - 1) as u64 == 0 {
                                self.phase = TestPhase::PutNoResultReceiveTime;
                                poll.registry().reregister(
                                    stream,
                                    self.token,
                                    Interest::READABLE,
                                )?;
                                return Ok(());
                            } else {
                                return Ok(());
                            }
                        }
                        Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                            debug!("Write would block, reregistering for write");
                            return Ok(());
                        }
                        Err(e) => {
                            debug!("Error in PutNoResultSendChunks: {}", e);
                            return Err(e.into());
                        }
                    }
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
