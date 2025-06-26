use crate::globals::{CHUNK_STORAGE, CHUNK_TERMINATION_STORAGE};
use crate::handlers::BasicHandler;
use crate::state::{MeasurementState, TestPhase};
use crate::stream::Stream;
use crate::{read_until, write_all_nb};
use anyhow::Result;
use bytes::BytesMut;
use fastrand;
use log::{debug, info, trace};
use mio::{net::TcpStream, Interest, Poll, Token};
use std::io::{self, Write};
use std::time::Instant;

const TEST_DURATION_NS: u64 = 7_000_000_000; // 3 seconds
const MAX_CHUNK_SIZE: u64 = 4194304; // 2MB

pub struct PerfHandler {
    token: Token,
    chunk_size: u64,
    test_start_time: Option<Instant>,
    bytes_sent: u64,
    write_buffer: BytesMut,
    read_buffer: BytesMut,
    upload_speed: Option<f64>,
}

impl PerfHandler {
    pub fn new(token: Token) -> Result<Self> {
        Ok(Self {
            token,
            chunk_size: MAX_CHUNK_SIZE,
            test_start_time: None,
            bytes_sent: 0,
            write_buffer: BytesMut::with_capacity(1024),
            read_buffer: BytesMut::with_capacity(1024),
            upload_speed: None,
        })
    }

    pub fn get_put_no_result_command(&self) -> String {
        format!("PUTNORESULT {}\n", self.chunk_size)
    }

    pub fn calculate_upload_speed(&mut self, time_ns: u64) -> f64 {
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
        debug!("Upload speed calculation:");
        debug!("  Total bytes sent: {}", bytes);
        debug!(
            " Total bytes sent GB: {}",
            bytes as f64 / (1024.0 * 1024.0 * 1024.0)
        );
        debug!("  Total time: {:.9} seconds ({} ns)", time_seconds, time_ns);
        debug!(
            "  Speed: {:.2} MB/s ({:.2} GB/s)  GBit/s: {}  Mbit/s: {}",
            speed_bps / (1024.0 * 1024.0),
            speed_bps / (1024.0 * 1024.0 * 1024.0),
            speed_bps * 8.0 / (1024.0 * 1024.0 * 1024.0),
            speed_bps * 8.0 / (1024.0 * 1024.0)
        );
        speed_bps
    }
}

impl BasicHandler for PerfHandler {
    fn on_read(
        &mut self,
        stream: &mut Stream,
        poll: &Poll,
        measurement_state: &mut MeasurementState,
    ) -> Result<()> {
        match measurement_state.phase {
            TestPhase::PerfNoResultReceiveOk => {
                debug!("PerfNoResultReceiveOk");
                loop {
                    let mut a = vec![0u8; 1024];
                    match stream.read(&mut a) {
                        Ok(n) => {
                            self.read_buffer.extend_from_slice(&a[..n]);
                            let time_str = String::from_utf8_lossy(&self.read_buffer);

                            if time_str.contains("OK\n") {
                                measurement_state.phase = TestPhase::PerfNoResultSendChunks;
                                stream.reregister(&poll, self.token, Interest::WRITABLE)?;
                                self.read_buffer.clear();
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
            }
            TestPhase::PerfNoResultSendChunks => {
                debug!("PerfNoResultSendChunks");
                let mut a = vec![0u8; 2048];
                loop {
                    match stream.read(&mut a) {
                        Ok(n) => {
                            self.read_buffer.extend_from_slice(&a[..n]);
                        }
                        Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                            return Ok(());
                        }
                        Err(e) => {
                            return Err(e.into());
                        }
                    }
                }
            }
            TestPhase::PerfNoResultReceiveTime => {
                debug!("PerfNoResultReceiveTime token {:?}", self.token);
                loop {
                    let mut a = vec![0u8; 1024*1024];
                    match stream.read(&mut a) {
                        Ok(n) => {
                            self.read_buffer.extend_from_slice(&a[..n]);
                            let line = String::from_utf8_lossy(&self.read_buffer);
                            debug!("Response: {}", line);
                            
                            // Ищем строку, которая начинается с TIME и заканчивается \n
                            if let Some(time_line) = line.lines().find(|l| l.trim().starts_with("TIME")) {
                                debug!("Time line: {} token {:?}", time_line, self.token);
                                let time_ns = time_line.split_whitespace().nth(1).and_then(|s| s.parse::<u64>().ok()).unwrap();
                                let speed_bps = self.calculate_upload_speed(time_ns);
                                measurement_state.upload_speed = Some(speed_bps);
                                measurement_state.upload_time = Some(time_ns);
                                measurement_state.upload_bytes = Some(self.bytes_sent);
                                measurement_state.phase = TestPhase::PerfNoResultCompleted;
                                stream.reregister(&poll, self.token, Interest::WRITABLE)?;
                                self.read_buffer.clear();
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
            }
            _ => {
                debug!("PerfNoResult on_read phase: {:?}", measurement_state.phase);
                return Ok(());
            }
        }
        Ok(())
    }

    fn on_write(
        &mut self,
        stream: &mut Stream,
        poll: &Poll,
        measurement_state: &mut MeasurementState,
    ) -> Result<()> {
        match measurement_state.phase {
            TestPhase::PerfNoResultSendCommand => {
                // debug!("PerfNoResultSendCommand");
                self.chunk_size = measurement_state.chunk_size as u64; 
                self.write_buffer
                    .extend_from_slice(self.get_put_no_result_command().as_bytes());
                if write_all_nb(&mut self.write_buffer, stream)? {
                    measurement_state.phase = TestPhase::PerfNoResultReceiveOk;
                    stream.reregister(&poll, self.token, Interest::READABLE)?;
                    return Ok(());
                }
                Ok(())
            }
            TestPhase::PerfNoResultSendChunks => {
                // debug!("PerfNoResultSendChunks");
                if self.test_start_time.is_none() {
                    self.test_start_time = Some(Instant::now());
                }
                if let Some(start_time) = self.test_start_time {
                    let current_pos = self.bytes_sent & (self.chunk_size as u64 - 1);
                    let buffer = CHUNK_STORAGE.get(&self.chunk_size).unwrap();
                    let mut remaining: &[u8] = &buffer[current_pos as usize..];
                    loop {
                        // Write from current position
                        match stream.write(remaining) {
                            Ok(written) => {
                                self.bytes_sent += written as u64;
                                remaining = &remaining[written..];
                                if remaining.is_empty() {
                                    let tt = start_time.elapsed().as_nanos();
                                    let is_last = tt >= TEST_DURATION_NS as u128;
                                    measurement_state
                                        .upload_measurements
                                        .push_back((tt as u64, self.bytes_sent));

                                    if is_last {
                                        debug!("Go to PerfNoResultSendLastChunk");
                                        measurement_state.phase =
                                            TestPhase::PerfNoResultSendLastChunk;
                                        stream.reregister(&poll, self.token, Interest::WRITABLE)?;
                                        return Ok(());
                                    } else {
                                        remaining = &buffer;
                                    }
                                }
                            }
                            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                                return Ok(());
                            }
                            Err(e) => {
                                trace!("PerfNoResultSendChunks error: {}", e);
                                return Err(e.into());
                            }
                        }
                    }
                } else {
                    return Ok(());
                }
            }

            TestPhase::PerfNoResultSendLastChunk => {
                debug!("PerfNoResultSendLastChunk");
                let current_pos = self.bytes_sent & (self.chunk_size as u64 - 1);
                let buffer = CHUNK_TERMINATION_STORAGE.get(&self.chunk_size).unwrap();
                let mut remaining = &buffer[current_pos as usize..];

                loop {
                    // Write from current position
                    match stream.write(remaining) {
                        Ok(written) => {
                            self.bytes_sent += written as u64;
                            remaining = &remaining[written..];
                            if remaining.is_empty() {
                                debug!("PerfNoResultSendLastChunk success token {:?}", self.token);
                                measurement_state.phase = TestPhase::PerfNoResultReceiveTime;
                                stream.reregister(
                                    &poll,
                                    self.token,
                                    Interest::READABLE | Interest::WRITABLE,
                                )?;
                                return Ok(());
                            }
                        }
                        Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                            debug!("PerfNoResultSendLastChunk WouldBlock");
                            return Ok(());
                        }
                        Err(e) => {
                            debug!("PerfNoResultSendLastChunk error: {}", e);
                            return Err(e.into());
                        }
                    }
                }

                // if !(stream.return_type() == "WebSocket") {
                //     stream.reregister(&poll, self.token, Interest::READABLE)?;
                // } else {
                // stream.reregister(
                //     &poll,
                //     self.token,
                //     Interest::READABLE | Interest::WRITABLE,
                // )?;
                // }
                Ok(())
            }
            _ => {
                debug!(
                    "Perf on write phase: {:?}",
                    measurement_state.phase
                );
                return self.on_read(stream, poll, measurement_state);
            }
        }
    }
}
