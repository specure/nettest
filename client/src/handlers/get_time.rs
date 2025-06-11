use crate::handlers::BasicHandler;
use crate::state::{MeasurementState, TestPhase};
use crate::stream::{OpenSslStream, Stream};
use crate::utils::ACCEPT_GETCHUNKS_STRING;
use crate::{read_until, write_all_nb};

use anyhow::Result;
use bytes::BytesMut;
use log::{debug, info, trace};
use mio::{net::TcpStream, Interest, Poll, Token};
use std::io::{self, Read};
use std::time::{Duration, Instant};

const TEST_DURATION_NS: u64 = 7_000_000_000; // 7 seconds
const MAX_CHUNK_SIZE: u32 = 4194304; // 4MB

pub struct GetTimeHandler {
    token: Token,
    chunk_size: u32,
    bytes_received: u64,
    read_buffer: BytesMut,
    write_buffer: BytesMut,
    responses: Vec<(u64, u64)>, // (time_ns, bytes)
    chunk_buffer: Vec<u8>,      // Буфер для чтения чанков
}

impl GetTimeHandler {
    pub fn new(token: Token) -> Result<Self> {
        Ok(Self {
            token,
            chunk_size: MAX_CHUNK_SIZE,
            bytes_received: 0,
            read_buffer: BytesMut::with_capacity(MAX_CHUNK_SIZE as usize),
            write_buffer: BytesMut::with_capacity(1024),
            responses: Vec::new(),
            chunk_buffer: vec![0u8; MAX_CHUNK_SIZE as usize],
        })
    }
    pub fn calculate_download_speed(&self, time_ns: u64) -> f64 {
        let bytes = self.bytes_received;
        // Convert nanoseconds to seconds, ensuring we don't lose precision
        let time_seconds = if time_ns > u64::MAX / 1_000_000_000 {
            // If time_ns is very large, divide first to avoid overflow
            (time_ns / 1_000_000_000) as f64 + (time_ns % 1_000_000_000) as f64 / 1_000_000_000.0
        } else {
            time_ns as f64 / 1_000_000_000.0
        };
        info!("time_seconds: {}", time_seconds);
        let speed_bps: f64 = bytes as f64 / time_seconds; // Convert to bytes per second
                                                          // self.upload_speed = Some(speed_bps);
        info!("Download speed calculation:");
        debug!("  Total bytes sent: {}", bytes);
        debug!(
            " Total bytes sent GB: {}",
            bytes as f64 / (1024.0 * 1024.0 * 1024.0)
        );
        debug!("  Total time: {:.9} seconds ({} ns)", time_seconds, time_ns);
        info!(
            "  Speed: {:.2} MB/s ({:.2} GB/s)  GBit/s: {}  Mbit/s: {}",
            speed_bps / (1024.0 * 1024.0),
            speed_bps / (1024.0 * 1024.0 * 1024.0),
            speed_bps * 8.0 / (1024.0 * 1024.0 * 1024.0),
            speed_bps * 8.0 / (1024.0 * 1024.0)
        );
        speed_bps
    }

    pub fn get_ok_command(&self) -> String {
        "OK\n".to_string()
    }
}

impl BasicHandler for GetTimeHandler {
    fn on_read(
        &mut self,
        stream: &mut Stream,
        poll: &Poll,
        measurement_state: &mut MeasurementState,
    ) -> Result<()> {
        // trace!("Reading chunk GetTimeHandler of size {}", self.chunk_size);
        match measurement_state.phase {
            TestPhase::GetTimeReceiveChunk => loop {
                        match stream.read(&mut self.chunk_buffer) {
                            Ok(n) if n > 0 => {
                                self.bytes_received += n as u64;

                                if self.chunk_buffer[n - 1] == 0xFF
                                    && (self.bytes_received & (self.chunk_size as u64 - 1)) == 0
                                {
                                    measurement_state.phase = TestPhase::GetTimeSendOk;
                                    stream.reregister(&poll, self.token, Interest::WRITABLE)?;
                                    return Ok(());
                                }
                            }
                            Ok(0) => {
                                return Err(io::Error::new(
                                    io::ErrorKind::ConnectionAborted,
                                    "Connection closed",
                                )
                                .into());
                            }
                            Ok(_) => {
                                return Ok(());
                            }
                            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                                return Ok(());
                            }
                            Err(e) => return Err(e.into()),
                        }
            },
            TestPhase::GetTimeReceiveTime => {
                if read_until(stream, &mut self.read_buffer, ACCEPT_GETCHUNKS_STRING)? {
                    if let Some(time_ns) = String::from_utf8_lossy(&self.read_buffer)
                        .split_whitespace()
                        .nth(1)
                        .and_then(|s| s.parse::<u64>().ok())
                    {
                        self.responses.push((time_ns, self.bytes_received));
                        measurement_state.download_time = Some(time_ns);
                        measurement_state.download_bytes = Some(self.bytes_received);
                        measurement_state.download_speed =
                            Some(self.calculate_download_speed(time_ns));
                        measurement_state.phase = TestPhase::PutNoResultSendCommand;
                        stream.reregister(&poll, self.token, Interest::WRITABLE)?;
                    }
                    self.read_buffer.clear();
                }
            }
            _ => {}
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
            TestPhase::GetTimeSendCommand => {
                self.chunk_size = measurement_state.chunk_size;
                if self.write_buffer.is_empty() {
                    self.write_buffer.extend_from_slice(
                        format!(
                            "GETTIME {} {}\n",
                            TEST_DURATION_NS / 1_000_000_000,
                            self.chunk_size
                        )
                        .as_bytes(),
                    );
                }
                if write_all_nb(&mut self.write_buffer, stream)? {
                    measurement_state.phase = TestPhase::GetTimeReceiveChunk;
                    stream.reregister(&poll, self.token, Interest::READABLE)?;
                    self.write_buffer.clear();
                }
            }
            TestPhase::GetTimeSendOk => {
                if self.write_buffer.is_empty() {
                    self.write_buffer
                        .extend_from_slice(self.get_ok_command().as_bytes());
                }
                if write_all_nb(&mut self.write_buffer, stream)? {
                    measurement_state.phase = TestPhase::GetTimeReceiveTime;
                    stream.reregister(&poll, self.token, Interest::READABLE)?;
                    self.write_buffer.clear();
                }
            }
            _ => {}
        }
        Ok(())
    }
}
