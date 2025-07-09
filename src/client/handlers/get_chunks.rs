use crate::client::globals::MIN_CHUNK_SIZE;
use crate::client::handlers::BasicHandler;
use crate::client::state::TestPhase;
use crate::client::utils::{
    ACCEPT_GETCHUNKS_STRING, MAX_CHUNKS_BEFORE_SIZE_INCREASE, MAX_CHUNK_SIZE, OK_COMMAND,
    PRE_DOWNLOAD_DURATION_NS, TIME_BUFFER_SIZE,
};
use crate::client::{write_all_nb, MeasurementState, Stream, DEFAULT_READ_BUFFER_SIZE};
use anyhow::Result;
use bytes::BytesMut;
use log::{debug, trace};
use mio::{Interest, Poll, Token};
use std::io::{self};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Instant;

pub struct GetChunksHandler {
    token: Token,
    chunk_size: u32,
    total_chunks: u32,
    chunks_received: AtomicU32,
    test_start_time: Option<Instant>,
    unprocessed_bytes: AtomicU32,
    time_buffer: BytesMut,
    write_buffer: BytesMut,
    chunk_buffer: Vec<u8>,
    cursor: usize,
}

impl GetChunksHandler {
    pub fn new(token: Token) -> Result<Self> {
        Ok(Self {
            token,
            chunk_size: MIN_CHUNK_SIZE as u32,
            total_chunks: 1,
            chunks_received: AtomicU32::new(0),
            test_start_time: None,
            unprocessed_bytes: AtomicU32::new(0),
            time_buffer: BytesMut::with_capacity(TIME_BUFFER_SIZE),
            write_buffer: BytesMut::with_capacity(DEFAULT_READ_BUFFER_SIZE),
            chunk_buffer: Vec::with_capacity(MAX_CHUNK_SIZE as usize),
            cursor: 0,
        })
    }

    fn increase_chunk_size(&mut self) {
        if self.total_chunks < MAX_CHUNKS_BEFORE_SIZE_INCREASE {
            self.total_chunks *= 2;
            trace!("Increased total chunks to {}", self.total_chunks);
        } else {
            self.chunk_size = (self.chunk_size * 2).min(MAX_CHUNK_SIZE);
            self.chunk_buffer.resize(self.chunk_size as usize, 0);
            trace!("Increased chunk size to {}", self.chunk_size);
        }
    }

    fn reset_chunk_counters(&mut self) {
        self.chunks_received.store(0, Ordering::SeqCst);
        self.unprocessed_bytes.store(0, Ordering::SeqCst);
        trace!("Reset chunk counters");
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
            TestPhase::GetChunksReceiveChunk => {
                debug!("GetChunksReceiveChunk");
                loop {
                    debug!("Reading chunk {} bytes n {}", self.chunk_buffer.len(), self.cursor);
                    match stream.read(&mut self.chunk_buffer[self.cursor..]) {
                        Ok(n) if n > 0 => {
                            self.cursor += n;
                            debug!("Chunk received {} bytes", n);
                            if self.cursor == self.chunk_size as usize {
                                debug!("Chunk received");
                                self.chunks_received.fetch_add(1, Ordering::SeqCst);
                                if self.chunk_buffer[self.cursor - 1] == 0x00 {
                                    debug!("Chunk received 0x00");
                                    self.cursor = 0;
                                } else if self.chunk_buffer[self.cursor - 1] == 0xFF {
                                    debug!("Chunk received 0xFF");
                                    measurement_state.phase = TestPhase::GetChunksSendOk;
                                    self.cursor = 0;
                                    stream.reregister(&poll, self.token, Interest::WRITABLE)?;
                                    return Ok(());
                                }
                                if self.cursor > 0 && self.chunk_buffer[self.cursor - 1] != 0x00 && self.chunk_buffer[self.cursor - 1] != 0xFF {
                                    let a = String::from_utf8_lossy(&self.chunk_buffer);
                                    debug!("Chunk received Invalid chunk: {}", a);
                                    return Err(anyhow::anyhow!("Chunk received Invalid chunk"));
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
                }
            }

            TestPhase::GetChunksReceiveTime => {
                trace!("GetChunksReceiveTime");
                loop {
                    let mut buf = [0u8; 1024];
                    match stream.read(&mut buf) {
                        Ok(0) => break, // EOF
                        Ok(n) => {
                            self.time_buffer.extend_from_slice(&buf[..n]);
                            let buffer_str = String::from_utf8_lossy(&self.time_buffer);

                            if buffer_str.contains(ACCEPT_GETCHUNKS_STRING) {
                                if let Some(time_ns) = self.parse_time_response(&buffer_str) {
                                    trace!("Time: {} ns", time_ns);
                                    if time_ns < PRE_DOWNLOAD_DURATION_NS
                                        && self.chunk_size < MAX_CHUNK_SIZE
                                    {
                                        self.increase_chunk_size();
                                        self.reset_chunk_counters();
                                        measurement_state.phase =
                                            TestPhase::GetChunksSendChunksCommand;
                                        stream.reregister(&poll, self.token, Interest::WRITABLE)?;
                                    } else {
                                        measurement_state.chunk_size = self.chunk_size as usize;
                                        measurement_state.phase = TestPhase::GetChunksCompleted;
                                        // stream.reregister(&poll, self.token, Interest::WRITABLE)?;
                                    }
                                }
                                self.time_buffer.clear();
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
                trace!("GetChunksSendChunksCommand");
                let command = format!("GETCHUNKS {} {}\n", self.total_chunks, self.chunk_size);
                self.write_buffer.extend_from_slice(command.as_bytes());
                if write_all_nb(&mut self.write_buffer, stream)? {
                    measurement_state.phase = TestPhase::GetChunksReceiveChunk;
                    self.unprocessed_bytes.store(0, Ordering::SeqCst);
                    self.chunks_received.store(0, Ordering::SeqCst);
                    self.chunk_buffer = vec![0u8; self.chunk_size as usize];

                    self.test_start_time = Some(Instant::now());
                    stream.reregister(&poll, self.token, Interest::READABLE)?;
                    self.write_buffer.clear();
                }
            }
            TestPhase::GetChunksSendOk => {
                trace!("GetChunksSendOk");
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
