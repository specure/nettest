use crate::handlers::BasicHandler;
use crate::state::{MeasurementState, TestPhase};
use crate::utils::utils::write_all_nb_loop;
use crate::{read_until, write_all_nb};
use crate::stream::Stream;
use anyhow::Result;
use bytes::{Buf, BytesMut};
use fastrand;
use log::{debug, info, trace};
use mio::{net::TcpStream, Interest, Poll, Token};
use std::io::{self, IoSlice, Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

const TEST_DURATION_NS: u64 = 7_000_000_000; // 7 seconds
const MAX_CHUNK_SIZE: u64 = 4194304; // 4MB
const BUFFER_SIZE: usize = 4 * 1024 * 1024; // 4mb

pub struct PutHandler {
    token: Token,
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
    fn on_read(
        &mut self,
        stream: &mut Stream,
        poll: &Poll,
        measurement_state: &mut MeasurementState,
    ) -> Result<()> {
        match measurement_state.phase {
            TestPhase::PutReceiveOk => {
                if read_until(stream, &mut self.read_buffer, "OK\n")? {
                    measurement_state.phase = TestPhase::PutSendChunks;
                    self.read_buffer.clear();
                    stream.reregister(&poll, self.token, Interest::WRITABLE)?;
                }
            }
            TestPhase::PutReceiveBytesTime => {
                if read_until(stream, &mut self.read_buffer, "\n")? {
                    if let Some(time_ns) = String::from_utf8_lossy(&self.read_buffer)
                        .split_whitespace()
                        .nth(1)
                        .and_then(|s| s.parse::<u64>().ok())
                    {
                        if let Some(bytes) = String::from_utf8_lossy(&self.read_buffer)
                            .split_whitespace()
                            .nth(3)
                            .and_then(|s| s.parse::<u64>().ok())
                        {
                            self.responses.push((time_ns, bytes));
                            // self.phase = TestPhase::PutSendChunks;
                            self.read_buffer.clear();
                            measurement_state.phase = TestPhase::PutSendChunks;

                            stream.reregister(&poll, self.token, Interest::WRITABLE)?;
                        }
                    }
                }
            }
            TestPhase::PutReceiveTime => {
                if read_until(stream, &mut self.read_buffer, "\n")? {
                    let line = String::from_utf8_lossy(&self.read_buffer);

                    if let Some(time_ns) = line
                        .split_whitespace()
                        .nth(1)
                        .and_then(|s| s.parse::<u64>().ok())
                    {
                        measurement_state
                            .upload_results_for_graph
                            .push((self.bytes_sent, time_ns));
                        measurement_state.phase = TestPhase::PutCompleted;
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
            TestPhase::PutSendCommand => {
                self.write_buffer
                    .extend_from_slice(self.get_put_command().as_bytes());

                if write_all_nb(&mut self.write_buffer, stream)? {
                    measurement_state.phase = TestPhase::PutReceiveOk;
                    stream.reregister(&poll, self.token, Interest::READABLE)?;
                    return Ok(());
                }
            }
            TestPhase::PutSendChunks => {
                if self.test_start_time.is_none() {
                    self.test_start_time = Some(Instant::now());
                }

                if let Some(start_time) = self.test_start_time {
                    let elapsed = start_time.elapsed();
                    let is_last = elapsed.as_nanos() >= TEST_DURATION_NS as u128;

                    if self.write_buffer.is_empty() {
                        let buffer = if is_last {
                            &self.data_termination_buffer
                        } else {
                            &self.data_buffer
                        };
                        self.write_buffer.extend_from_slice(buffer);
                    }
                    if !is_last {
                        if write_all_nb_loop(&mut self.write_buffer, stream)? {
                            measurement_state.phase = TestPhase::PutReceiveBytesTime;
                            stream.reregister(&poll, self.token, Interest::READABLE)?;
                            return Ok(());
                        } 
                    } else {
                        if write_all_nb_loop(&mut self.write_buffer, stream)? {
                            measurement_state.phase = TestPhase::PutReceiveTime;
                            stream.reregister(&poll, self.token, Interest::READABLE)?;
                            return Ok(());
                        }
                    }
                }
            }
            _ => {
                debug!("PutHandler phase {:?}", measurement_state.phase);
            }
        }
        Ok(())
    }
}
