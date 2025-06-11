use crate::handlers::BasicHandler;
use crate::state::{MeasurementState, TestPhase};
use crate::{read_until, write_all_nb};
use crate::stream::Stream;
use anyhow::Result;
use bytes::{BytesMut};
use fastrand;
use log::{debug, info, trace};
use mio::{net::TcpStream, Interest, Poll, Token};
use std::io::{self, Write};
use std::time::{Instant};

const TEST_DURATION_NS: u64 = 7_000_000_000; // 3 seconds
const MAX_CHUNK_SIZE: u64 = 4194304; // 2MB

pub struct PutNoResultHandler {
    token: Token,
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
        let mut data_buffer = vec![0u8; MAX_CHUNK_SIZE as usize];
        for byte in &mut data_buffer {
            *byte = fastrand::u8(..);
        }
        data_buffer[MAX_CHUNK_SIZE as usize - 1] = 0x00;
        let mut data_termination_buffer = vec![0u8; MAX_CHUNK_SIZE as usize];
        for byte in &mut data_termination_buffer {
            *byte = fastrand::u8(..);
        }
        data_termination_buffer[MAX_CHUNK_SIZE as usize - 1] = 0xFF;

        Ok(Self {
            token,
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

    fn calculate_upload_speed(&mut self, time_ns: u64) -> f64 {
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

impl BasicHandler for PutNoResultHandler {
    fn on_read(&mut self, stream: &mut Stream, poll: &Poll, measurement_state: &mut MeasurementState) -> Result<()> {
        match measurement_state.phase {
            TestPhase::PutNoResultReceiveOk => {
                if read_until(stream, &mut self.read_buffer, "OK\n")? {
                    measurement_state.phase = TestPhase::PutNoResultSendChunks;
                    stream.reregister(&poll, self.token, Interest::WRITABLE)?;
                    self.read_buffer.clear();
                    return Ok(());
                }
            }
            TestPhase::PutNoResultReceiveTime => {
                if read_until(
                    stream,
                    &mut self.read_buffer,
                    "ACCEPT GETCHUNKS GETTIME PUT PUTNORESULT PING QUIT\n",
                )? {
                    let line: std::borrow::Cow<'_, str> =
                        String::from_utf8_lossy(&self.read_buffer);
                    if let Some(time_line) = line.lines().find(|l| l.starts_with("TIME")) {
                        if let Some(time_ns) = time_line
                            .split_whitespace()
                            .nth(1)
                            .and_then(|s| s.parse::<u64>().ok())
                        {
                            let speed_bps = self.calculate_upload_speed(time_ns);
                            measurement_state.upload_speed = Some(speed_bps);
                            measurement_state.upload_time = Some(time_ns);
                            measurement_state.upload_bytes = Some(self.bytes_sent);
                        }
                    }
                    measurement_state.phase = TestPhase::PutSendCommand;
                    stream.reregister(&poll, self.token, Interest::WRITABLE)?;
                    self.read_buffer.clear();
                }
            }
            _ => {
                return Ok(());
            }
        }
        Ok(())
    }

    fn on_write(&mut self, stream: &mut Stream, poll: &Poll, measurement_state: &mut MeasurementState) -> Result<()> {
        match measurement_state.phase {
            TestPhase::PutNoResultSendCommand => {
                self.write_buffer
                    .extend_from_slice(self.get_put_no_result_command().as_bytes());
                if write_all_nb(&mut self.write_buffer, stream)? {
                    measurement_state.phase = TestPhase::PutNoResultReceiveOk;
                    stream.reregister(&poll, self.token, Interest::READABLE)?;
                    return Ok(());
                }
                Ok(())
            }
            TestPhase::PutNoResultSendChunks => {
                if self.test_start_time.is_none() {
                    self.test_start_time = Some(Instant::now());
                } 
                if let Some(start_time) = self.test_start_time {
                    let elapsed = start_time.elapsed();
                    let mut is_last = elapsed.as_nanos() >= TEST_DURATION_NS as u128;

                    let current_pos = self.bytes_sent & (self.chunk_size as u64 - 1);
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
                            // trace!("Elapsed: {} ns", elapsed.as_nanos());
                            is_last = start_time.elapsed().as_nanos() >= TEST_DURATION_NS as u128;

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
                                return Ok(());
                            }
                            Err(e) => {
                                return Err(e.into());
                            }
                        }
                    }
                    measurement_state.phase = TestPhase::PutNoResultReceiveTime;
                    stream.reregister(&poll, self.token, Interest::READABLE)?;
                    return Ok(());
                } else {
                    return Ok(());
                }
            }
            _ => {
                return Ok(());
            }
        }
    }
}
