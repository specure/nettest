use crate::handlers::BasicHandler;
use crate::state::{MeasurementState, TestPhase};
use crate::utils::{
    read_until, ACCEPT_GETCHUNKS_STRING, DEFAULT_READ_BUFFER_SIZE, MAX_CHUNKS_BEFORE_SIZE_INCREASE,
    MAX_CHUNK_SIZE, MIN_CHUNK_SIZE, OK_COMMAND, PRE_DOWNLOAD_DURATION_NS, TIME_BUFFER_SIZE,
};
use crate::{write_all_nb};
use crate::stream::Stream;
use anyhow::Result;
use bytes::BytesMut;
use log::{debug, trace};
use mio::{net::TcpStream, Interest, Poll, Token};
use std::io::{self, Read};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Instant;

pub struct GetChunksHandler {
    token: Token,
    chunk_size: u32,
    total_chunks: u32,
    chunks_received: AtomicU32,
    test_start_time: Option<Instant>,
    unprocessed_bytes: AtomicU32,
    read_buffer: BytesMut,
    time_buffer: BytesMut,
    write_buffer: BytesMut,
}

impl GetChunksHandler {
    pub fn new(token: Token) -> Result<Self> {
        Ok(Self {
            token,
            chunk_size: MIN_CHUNK_SIZE,
            total_chunks: 1, // Start with 1 chunk as per specification
            chunks_received: AtomicU32::new(0),
            test_start_time: None,
            unprocessed_bytes: AtomicU32::new(0),
            read_buffer: BytesMut::with_capacity(DEFAULT_READ_BUFFER_SIZE),
            time_buffer: BytesMut::with_capacity(TIME_BUFFER_SIZE),
            write_buffer: BytesMut::with_capacity(DEFAULT_READ_BUFFER_SIZE),
        })
    }

    fn increase_chunk_size(&mut self) {
        if self.total_chunks < MAX_CHUNKS_BEFORE_SIZE_INCREASE {
            self.total_chunks *= 2;
            debug!("Increased total chunks to {}", self.total_chunks);
        } else {
            self.chunk_size = (self.chunk_size * 2).min(MAX_CHUNK_SIZE);
            debug!("Increased chunk size to {}", self.chunk_size);
        }
    }

    fn reset_chunk_counters(&mut self) {
        self.chunks_received.store(0, Ordering::SeqCst);
        self.unprocessed_bytes.store(0, Ordering::SeqCst);
        debug!("Reset chunk counters");
    }

    fn parse_time_response(&self, buffer_str: &str) -> Option<u64> {
        buffer_str
            .find("TIME ")
            .and_then(|time_start| {
                buffer_str[time_start..]
                    .find('\n')
                    .map(|time_end| &buffer_str[time_start + 5..time_start + time_end])
            })
            .and_then(|time_str| time_str.parse::<u64>().ok())
    }
}

impl BasicHandler for GetChunksHandler {
    fn on_read(
        &mut self,
        stream: &mut Stream,
        poll: &Poll,
        measurement_state: &mut MeasurementState,
    ) -> Result<()> {
        match measurement_state.phase {
            TestPhase::GetChunksReceiveAccept => {
                if read_until(stream, &mut self.read_buffer, ACCEPT_GETCHUNKS_STRING)? {
                    measurement_state.phase = TestPhase::GetChunksSendChunksCommand;
                    stream.reregister(&poll, self.token, Interest::WRITABLE)?;
                    self.read_buffer.clear();
                }
            }
            TestPhase::GetChunksReceiveChunk => {
                let mut buf: Vec<u8> = vec![0u8; self.chunk_size as usize];
                debug!("Reading chunk of size {}", self.chunk_size);
                loop {
                    match stream.read(&mut buf) {
                        Ok(n) if n > 0 => {
                            trace!("Read {} bytes of chunk", n);
                            let current_unprocessed =
                                self.unprocessed_bytes.fetch_add(n as u32, Ordering::SeqCst);
                            let total_bytes = current_unprocessed + n as u32;
                            let complete_chunks = total_bytes / self.chunk_size;
                            self.unprocessed_bytes
                                .store(total_bytes % self.chunk_size, Ordering::SeqCst);
                            let current_chunks = self
                                .chunks_received
                                .fetch_add(complete_chunks, Ordering::SeqCst);
                            if current_chunks + complete_chunks >= self.total_chunks {
                                let is_last_chunk = buf[n - 1] == 0xFF;
                                if is_last_chunk {
                                    debug!(
                                        "Received last chunk {}",
                                        current_chunks + complete_chunks
                                    );
                                    measurement_state.phase = TestPhase::GetChunksSendOk;
                                    stream.reregister(&poll, self.token, Interest::WRITABLE)?;
                                    return Ok(());
                                }
                            }
                        }
                        Ok(_) => {
                            debug!("Read 0 bytes");
                            return Ok(());
                        }
                        Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                            debug!("WouldBlock GetChunksReceiveChunk");
                            return Ok(());
                        }
                        Err(e) => return Err(e.into()),
                    }
                }
            }
            TestPhase::GetChunksReceiveTime => {
                if read_until(stream, &mut self.time_buffer, ACCEPT_GETCHUNKS_STRING)? {
                    let buffer_str = String::from_utf8_lossy(&self.time_buffer);
                    if let Some(time_ns) = self.parse_time_response(&buffer_str) {
                        debug!("Time: {} ns", time_ns);
                        if time_ns < PRE_DOWNLOAD_DURATION_NS && self.chunk_size < MAX_CHUNK_SIZE {
                            self.increase_chunk_size();
                            self.reset_chunk_counters();
                            measurement_state.phase = TestPhase::GetChunksSendChunksCommand;
                            stream.reregister(&poll, self.token, Interest::WRITABLE)?;
                        } else {
                            measurement_state.chunk_size = self.chunk_size;
                            measurement_state.phase = TestPhase::PingSendPing;
                            stream.reregister(&poll, self.token, Interest::WRITABLE)?;
                        }
                    }
                    self.time_buffer.clear();
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
            TestPhase::GetChunksSendChunksCommand => {
                let command = format!("GETCHUNKS {} {}\n", self.total_chunks, self.chunk_size);
                self.write_buffer.extend_from_slice(command.as_bytes());
                if write_all_nb(&mut self.write_buffer, stream)? {
                    measurement_state.phase = TestPhase::GetChunksReceiveChunk;
                    self.unprocessed_bytes.store(0, Ordering::SeqCst);
                    self.chunks_received.store(0, Ordering::SeqCst);
                    self.test_start_time = Some(Instant::now());
                    stream.reregister(&poll, self.token, Interest::READABLE)?;
                    self.write_buffer.clear();
                }
            }
            TestPhase::GetChunksSendOk => {
                if self.write_buffer.is_empty() {
                    self.write_buffer.extend_from_slice(
                        String::from_utf8_lossy(OK_COMMAND).to_string().as_bytes(),
                    );
                }
                if write_all_nb(&mut self.write_buffer, stream)? {
                    measurement_state.phase = TestPhase::GetChunksReceiveTime;
                    self.write_buffer = BytesMut::with_capacity(TIME_BUFFER_SIZE);
                    if self.write_buffer.is_empty() {
                        stream.reregister(&poll, self.token, Interest::READABLE)?;
                    }
                    self.write_buffer.clear();
                }
            }
            _ => {}
        }
        Ok(())
    }
}
