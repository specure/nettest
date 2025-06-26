use crate::handlers::BasicHandler;
use crate::state::{MeasurementState, TestPhase};
use crate::stream::Stream;
use crate::utils::ACCEPT_GETCHUNKS_STRING;
use crate::{read_until, write_all_nb};

use anyhow::Result;
use bytes::BytesMut;
use log::{debug, info, trace};
use mio::{Interest, Poll, Token};
use std::io::{self, Read};
use std::time::{Duration, Instant};

const TEST_DURATION_NS: u64 = 7_000_000_000; // 7 seconds
const MAX_CHUNK_SIZE: u32 = 4194304; // 4MB

pub struct GetTimeHandler {
    token: Token,
    chunk_size: usize,
    bytes_received: u64,
    start_time: Instant,
    read_buffer: BytesMut,
    write_buffer: BytesMut,
    chunk_buffer: Vec<u8>, 
    cursor: usize,     // Буфер для чтения чанков
}

impl GetTimeHandler {
    pub fn new(token: Token) -> Result<Self> {
        Ok(Self {
            token,
            chunk_size: MAX_CHUNK_SIZE as usize,
            bytes_received: 0,
            read_buffer: BytesMut::with_capacity(MAX_CHUNK_SIZE as usize),
            write_buffer: BytesMut::with_capacity(1024),
            chunk_buffer: Vec::with_capacity(MAX_CHUNK_SIZE as usize),
            start_time: Instant::now(),
            cursor: 0,
        })
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
        match measurement_state.phase {
            TestPhase::GetTimeReceiveChunk => loop {
                    match stream.read(&mut self.chunk_buffer[self.cursor..]) {
                        Ok(n) if n > 0 => {
                            self.cursor += n;
                            self.bytes_received += n as u64;
                            if self.cursor == self.chunk_size {
                                if self.chunk_buffer[self.cursor - 1] == 0x00 {
                                    measurement_state.measurements.push_back((
                                        self.start_time.elapsed().as_nanos() as u64,
                                        self.bytes_received,
                                    ));
                                    self.cursor = 0;
                                } else
                                if self.chunk_buffer[self.cursor - 1] == 0xFF {
                                    measurement_state.phase = TestPhase::GetTimeSendOk;
                                    stream.reregister(&poll, self.token, Interest::WRITABLE)?;
                                    return Ok(());
                                } 
                            }
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
                        info!("Time: {} ns",  String::from_utf8_lossy(&self.read_buffer));
                        let speed = self.bytes_received as f64 / time_ns as f64;
                        measurement_state.download_speed = Some(speed);
                        measurement_state.download_time = Some(time_ns);
                        measurement_state.download_bytes = Some(self.bytes_received);
                        
                        debug!("Speed: {}", speed);
                        measurement_state.phase = TestPhase::GetTimeCompleted;
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
                self.chunk_buffer = vec![0u8; self.chunk_size];
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
                self.start_time = Instant::now();
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
