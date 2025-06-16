use crate::globals::{CHUNK_STORAGE, CHUNK_TERMINATION_STORAGE};
use crate::handlers::BasicHandler;
use crate::state::{MeasurementState, TestPhase};
use crate::utils::{ACCEPT_GETCHUNKS_STRING, MAX_CHUNKS_BEFORE_SIZE_INCREASE};
use crate::{read_until, write_all_nb};
use crate::stream::Stream;
use anyhow::Result;
use bytes::{BytesMut};
use fastrand;
use log::{debug, info, trace};
use mio::{net::TcpStream, Interest, Poll, Token};
use std::io::{self, Write, ErrorKind};
use std::time::{Instant};

const TEST_DURATION_NS: u64 = 2_000_000_000; // 2 seconds
const MIN_CHUNK_SIZE: u64 = 4096; // 4KB
const MAX_CHUNK_SIZE: u64 = 4194304; // 4MB

pub struct PutNoResultHandler {
    token: Token,
    chunk_size: u64,
    total_chunks: u32,
    chunks_sent: u32,
    test_start_time: Option<Instant>,
    bytes_sent: u64,
    write_buffer: BytesMut,
    read_buffer: BytesMut,
    current_chunk_offset: usize,
    upload_speed: Option<f64>,
}

impl PutNoResultHandler {
    pub fn new(token: Token) -> Result<Self> {
        Ok(Self {
            token,
            chunk_size: MIN_CHUNK_SIZE,
            total_chunks: 1,
            chunks_sent: 0,
            test_start_time: None,
            bytes_sent: 0,
            write_buffer: BytesMut::with_capacity(1024),
            read_buffer: BytesMut::with_capacity(1024),
            current_chunk_offset: 0,
            upload_speed: None,
        })
    }

    pub fn get_put_no_result_command(&self) -> String {
        format!("PUTNORESULT {}\n", self.chunk_size)
    }

    fn increase_chunk_size(&mut self) {
        if self.total_chunks < MAX_CHUNKS_BEFORE_SIZE_INCREASE {
            self.total_chunks *= 2;
            trace!("Increased total chunks to {}", self.total_chunks);
        } else {
            self.chunk_size = (self.chunk_size * 2).min(MAX_CHUNK_SIZE);
            trace!("Increased chunk size to {}", self.chunk_size);
        }
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
                if read_until(stream, &mut self.read_buffer, ACCEPT_GETCHUNKS_STRING)? {
                    let time_str = String::from_utf8_lossy(&self.read_buffer);
                    
                    debug!("Received time: {:?}", time_str);
                    if let Some(time_ns) = time_str
                        .split_whitespace()
                        .nth(1)
                        .and_then(|s| s.parse::<u64>().ok())
                    {
                        self.read_buffer.clear();
                        if time_ns < TEST_DURATION_NS && self.chunk_size < MAX_CHUNK_SIZE {
                            self.increase_chunk_size();
                            self.chunks_sent = 0;
                            measurement_state.phase = TestPhase::PutNoResultSendCommand;
                            stream.reregister(&poll, self.token, Interest::WRITABLE)?;
                        } else {
                            measurement_state.phase = TestPhase::PutNoResultCompleted;
                            stream.reregister(&poll, self.token, Interest::WRITABLE)?;
                        }
                    }
                    self.read_buffer.clear();
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn on_write(&mut self, stream: &mut Stream, poll: &Poll, measurement_state: &mut MeasurementState) -> Result<()> {
        match measurement_state.phase {
            TestPhase::PutNoResultSendCommand => {
                debug!("Sending command: {:?}", self.get_put_no_result_command());
                self.write_buffer
                    .extend_from_slice(self.get_put_no_result_command().as_bytes());
                if write_all_nb(&mut self.write_buffer, stream)? {
                    debug!("Sent command: {:?}", self.get_put_no_result_command());
                    measurement_state.phase = TestPhase::PutNoResultReceiveOk;
                    stream.reregister(&poll, self.token, Interest::READABLE)?;
                    self.write_buffer.clear();
                    return Ok(());
                }
            }
            TestPhase::PutNoResultSendChunks => {
                if self.test_start_time.is_none() {
                    self.test_start_time = Some(Instant::now());
                }

                let is_last_chunk = self.chunks_sent == self.total_chunks - 1;
                let chunk = if is_last_chunk {
                    CHUNK_TERMINATION_STORAGE.get(&self.chunk_size)
                } else {
                    CHUNK_STORAGE.get(&self.chunk_size)
                };

                if let Some(chunk) = chunk {
                    let remaining = &chunk[self.current_chunk_offset..];
                    match stream.write(remaining) {
                        Ok(written) => {
                            self.current_chunk_offset += written;
                            self.bytes_sent += written as u64;

                            if self.current_chunk_offset == chunk.len() {
                                // Чанк полностью записан
                                self.chunks_sent += 1;
                                self.current_chunk_offset = 0;

                                if is_last_chunk {
                                    measurement_state.phase = TestPhase::PutNoResultReceiveTime;
                                    stream.reregister(&poll, self.token, Interest::READABLE)?;
                                }
                            }
                        }
                        Err(ref e) if e.kind() == ErrorKind::WouldBlock => {
                            // Сохраняем прогресс и ждем следующего write event
                            return Ok(());
                        }
                        Err(e) => {
                            return Err(e.into());
                        }
                    }
                } else {
                    return Err(anyhow::anyhow!("No chunk data found for size {}", self.chunk_size));
                }
            }
            _ => {}
        }
        Ok(())
    }
}
