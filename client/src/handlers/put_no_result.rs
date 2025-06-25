use crate::globals::{CHUNK_STORAGE, CHUNK_TERMINATION_STORAGE};
use crate::handlers::BasicHandler;
use crate::state::{MeasurementState, TestPhase};
use crate::stream::Stream;
use crate::utils::{ACCEPT_GETCHUNKS_STRING, MAX_CHUNKS_BEFORE_SIZE_INCREASE};
use crate::{read_until, write_all_nb};
use anyhow::Result;
use bytes::{Buf, BytesMut};
use fastrand;
use log::{debug, info, trace};
use mio::{net::TcpStream, Interest, Poll, Token};
use serde_json::de;
use std::io::{self, ErrorKind, Write};
use std::time::Instant;

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
            trace!("Increased total chunks to {} token {:?}", self.total_chunks, self.token);
        } else {
            self.chunk_size = (self.chunk_size * 2).min(MAX_CHUNK_SIZE);
            trace!("Increased chunk size to {} token {:?}", self.chunk_size, self.token);
        }
    }
}

impl BasicHandler for PutNoResultHandler {
    fn on_read(
        &mut self,
        stream: &mut Stream,
        poll: &Poll,
        measurement_state: &mut MeasurementState,
    ) -> Result<()> {
        match measurement_state.phase {
            TestPhase::PutNoResultReceiveOk => loop {
                debug!("PutNoResultReceiveOk token {:?}", self.token);
                if (self.test_start_time.is_none()) {
                    self.test_start_time = Some(Instant::now());
                }
                let mut a = vec![0u8; 1024];

                match stream.read(&mut a) {
                    Ok(n) => {
                        self.read_buffer.extend_from_slice(&a[..n]);

                        let line = String::from_utf8_lossy(&self.read_buffer);

                        if line.contains("OK\n") {
                            self.write_buffer.clear();
                            debug!("Read PutNoResultReceiveOk {} token {:?}", line, self.token);

                            measurement_state.phase = TestPhase::PutNoResultSendChunks;
                            stream.reregister(&poll, self.token, Interest::WRITABLE)?;
                            self.read_buffer.clear();
                            return Ok(());
                        }
                    }
                    Err(ref e) if e.kind() == ErrorKind::WouldBlock => {
                        debug!("WouldBlock token {:?}", self.token);
                        return Ok(());
                    }
                    Err(e) => {
                        debug!("Error token {:?}", self.token);
                        return Err(e.into());
                    }
                }
            },
            TestPhase::PutNoResultReceiveTime => {
                trace!("PutNoResultReceiveTime token {:?}", self.token);
                loop {
                    let mut a = vec![0u8; 1024];
                    match stream.read(&mut a) {
                        Ok(n) => {
                            self.read_buffer.extend_from_slice(&a[..n]);
                            let time_str = String::from_utf8_lossy(&self.read_buffer);
                            debug!("Read PutNoResultReceiveTime token afer read {:?} {:?}", self.token, time_str);


                            if time_str.contains(ACCEPT_GETCHUNKS_STRING) {
                                trace!("Received trace:  {:?} token {:?}", time_str, self.token);
                                
                                if let Some(time_ns) = time_str
                                    .split_whitespace()
                                    .nth(1)
                                    .and_then(|s| s.parse::<u64>().ok())
                                {
                                    self.read_buffer.clear();
                                    let time = self.test_start_time.unwrap().elapsed().as_nanos();
                                    if time  < TEST_DURATION_NS as u128
                                        && self.chunk_size < MAX_CHUNK_SIZE
                                    {
                                        trace!("Increasing chunk size");
                                        self.increase_chunk_size();
                                        self.chunks_sent = 0;
                                        measurement_state.phase = TestPhase::PutNoResultSendCommand;
                                        stream.reregister(&poll, self.token, Interest::WRITABLE)?;
                                    } else {
                                        trace!("Completed token {:?}", self.token);
                                        measurement_state.phase = TestPhase::PutNoResultCompleted;
                                        stream.reregister(&poll, self.token, Interest::WRITABLE)?;
                                    }
                                }
                            } else {
                                stream.reregister(&poll, self.token, Interest::READABLE)?;
                            }
                        }
                        Err(ref e) if e.kind() == ErrorKind::WouldBlock => {
                            trace!("WouldBlock token {:?}", self.token);
                            return Ok(());
                        }
                        Err(e) => {
                            return Err(e.into());
                        }
                    }
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
            TestPhase::PutNoResultSendCommand => {
                debug!("PutNoResultSendCommand token {:?}", self.token);
                if self.write_buffer.is_empty() {
                    self.write_buffer.extend_from_slice(self.get_put_no_result_command().as_bytes());
                }
                loop {
                    trace!("Sending command: {:?} token {:?}", self.get_put_no_result_command(), self.token);

                    match stream.write(&self.write_buffer) {
                        Ok(written) => {
                            self.write_buffer.advance(written);
                            if self.write_buffer.is_empty() {
                                measurement_state.phase = TestPhase::PutNoResultReceiveOk;
                                self.read_buffer.clear();
                                stream.reregister(&poll, self.token, Interest::READABLE)?;
                                self.write_buffer.clear(); 
                                return Ok(());
                            }
                        }
                        Err(ref e) if e.kind() == ErrorKind::WouldBlock => {
                            return Ok(());
                        }
                        Err(e) => {
                            return Err(e.into());
                        }
                    }
                }
            }
            TestPhase::PutNoResultSendChunks => {
                if self.test_start_time.is_none() {
                    self.test_start_time = Some(Instant::now());
                }

                let is_last_chunk = self.chunks_sent == self.total_chunks - 1;
                debug!("Sending total token {:?}", self.total_chunks);
                let chunk = if is_last_chunk {
                    trace!("Sending last chunk token {:?}", self.token);
                    CHUNK_TERMINATION_STORAGE.get(&self.chunk_size)
                } else {
                    debug!("Sending chunks token {:?}", self.token);

                    CHUNK_STORAGE.get(&self.chunk_size)
                };

                let mut remaining: &[u8] = &chunk.unwrap()[self.current_chunk_offset..];

                loop {
                  

                    if let Some(chunk) = chunk {
                        debug!("Sending remaining  {:?}", remaining.len());
                        debug!("Sending current chunk offset  {:?}", self.current_chunk_offset);
                        match stream.write(remaining) {
                            Ok(written) => {
                                remaining.advance(written);
                                self.current_chunk_offset += written;
                                self.bytes_sent += written as u64;

                                debug!("Sending written  {:?}", written);

                                if self.current_chunk_offset == chunk.len() {
                                    debug!("Equal token {:?}", self.token);
                                    // Чанк полностью записан
                                    self.chunks_sent += 1;
                                    self.current_chunk_offset = 0;

                                    if is_last_chunk {
                                        trace!("Last chunk sent token {:?}", self.token);
                                        
                                        measurement_state.phase = TestPhase::PutNoResultReceiveTime;
                                        stream.reregister(&poll, self.token, Interest::READABLE)?;
                                        return Ok(());
                                    }
                                    return Ok(());
                                }
                            }
                            Err(ref e) if e.kind() == ErrorKind::WouldBlock => {
                                // debug!("WouldBlock token {:?}", self.token);
                                // Сохраняем прогресс и ждем следующего write event
                                return Ok(());
                            }
                            Err(e) => {
                                return Err(e.into());
                            }
                        }
                    }
                    else {
                        debug!("No chunk token {:?}", self.token);
                        return Ok(());
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }
}
