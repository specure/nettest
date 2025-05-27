use crate::handlers::BasicHandler;
use crate::state::TestPhase;
use anyhow::Result;
use bytes::{Buf, BytesMut};
use log::debug;
use mio::{net::TcpStream, Interest, Poll, Token};
use std::io::{self, Read, Write};
use std::time::{Duration, Instant};

const MIN_CHUNK_SIZE: u32 = 4096; // 4KB
const MAX_CHUNK_SIZE: u32 = 4194304; // 4MB
const PRE_DOWNLOAD_DURATION_NS: u64 = 2_000_000_000; // 2 seconds

pub struct GetChunksHandler {
    token: Token,
    phase: TestPhase,
    chunk_size: u32,
    total_chunks: u32,
    chunks_received: u32,
    test_start_time: Option<Instant>,
    unprocessed_bytes: usize, // Track unprocessed bytes from previous read
    time_buffer: Vec<u8>, // Buffer for TIME command
}

impl GetChunksHandler {
    pub fn new(token: Token) -> Result<Self> {
        Ok(Self {
            token,
            phase: TestPhase::GetChunksReceiveAccept,
            chunk_size: MIN_CHUNK_SIZE,
            total_chunks: 8, // Start with 8 chunks as per specification
            chunks_received: 0,
            test_start_time: None,
            unprocessed_bytes: 0,
            time_buffer: Vec::new(),
        })
    }

    pub fn get_chunks_command(&self) -> String {
        format!("GETCHUNKS {} {}\n", self.total_chunks, self.chunk_size)
    }

    pub fn get_ok_command(&self) -> String {
        "OK\n".to_string()
    }
}

impl BasicHandler for GetChunksHandler {
    fn on_read(&mut self, stream: &mut TcpStream, poll: &Poll) -> Result<()> {
        let mut buf = vec![0u8; self.chunk_size as usize];
        debug!("[on_read] Reading from stream");
        match stream.read(&mut buf) {
            Ok(n) if n > 0 => {
                debug!("[on_read] Read {} bytes", n);
                match self.phase {
                    TestPhase::GetChunksReceiveAccept => {
                        if let Some(response) = String::from_utf8(buf[..n].to_vec()).ok() {
                            if response.contains("ACCEPT") {
                                //TODO: parse  Received ACCEPT command OK
                                // CHUNKSIZE 4096 4096 4194304
                                // ACCEPT GETCHUNKS GETTIME PUT PUTNORESULT PING QUIT
                                debug!("[on_read] Received ACCEPT command {}", response);
                                self.phase = TestPhase::GetChunksSendChunksCommand;
                                poll.registry().reregister(
                                    stream,
                                    self.token,
                                    Interest::WRITABLE,
                                )?;
                                self.test_start_time = Some(Instant::now());
                            }
                        }
                    }
                    TestPhase::GetChunksReceiveChunk => {
                        // Add unprocessed bytes from previous read to current buffer
                        let total_bytes = self.unprocessed_bytes + n;
                        let chunk_size = self.chunk_size as usize;

                        // Calculate how many complete chunks we have
                        let complete_chunks = total_bytes / chunk_size;
                        let remaining_bytes = total_bytes % chunk_size;

                        debug!(
                            "[on_read] Processing {} complete chunks, {} remaining bytes",
                            complete_chunks, remaining_bytes
                        );

                        // Process complete chunks
                        for i in 0..complete_chunks {
                            let chunk_start = i * chunk_size;
                            let chunk_end = chunk_start + chunk_size;

                            // Check if this is the last chunk (has termination byte 0xFF)
                            let is_last_chunk = buf[chunk_end - 1] == 0xFF;

                            if is_last_chunk {
                                debug!("[on_read] Received last chunk");
                                self.chunks_received += 1;
                                // Send OK command after receiving the last chunk
                                self.phase = TestPhase::GetChunksSendOk;
                                poll.registry().reregister(
                                    stream,
                                    self.token,
                                    Interest::WRITABLE,
                                )?;
                                self.unprocessed_bytes = remaining_bytes;

                                return Ok(()); // Return immediately after changing phase
                            } else {
                                self.chunks_received += 1;
                                self.unprocessed_bytes = remaining_bytes;
                                poll.registry().reregister(
                                    stream,
                                    self.token,
                                    Interest::READABLE,
                                )?;
                                debug!(
                                    "[on_read] Received chunk {}/{}",
                                    self.chunks_received, self.total_chunks
                                );
                            }
                        }
                    }
                    TestPhase::GetChunksReceiveTime => {
                        // Add new data to buffer
                        self.time_buffer.extend_from_slice(&buf[..n]);
                        if let Some(pos) = self.time_buffer.windows(1).position(|w| w == b"\n") {
                            let line = String::from_utf8_lossy(&self.time_buffer[..pos]);
                            if line.starts_with("TIME") {
                                debug!("[on_read] Received TIME response");
                                if let Some(time_ns) = line
                                    .split_whitespace()
                                    .nth(1)
                                    .and_then(|s| s.parse::<u64>().ok())
                                {
                                    debug!("[on_read] Time: {} ns", time_ns);
                                    // Double the number of chunks for next iteration if we haven't exceeded duration
                                    if let Some(start_time) = self.test_start_time {
                                        let elapsed = start_time.elapsed();
                                        if elapsed.as_nanos() < PRE_DOWNLOAD_DURATION_NS as u128 && self.chunk_size < MAX_CHUNK_SIZE {
                                            if self.total_chunks >= 8 {
                                                // If we've reached 8 chunks, start increasing chunk size instead
                                                self.chunk_size =
                                                    (self.chunk_size * 2).min(MAX_CHUNK_SIZE);
                                                debug!(
                                                    "[on_read] Increased chunk size to {}",
                                                    self.chunk_size
                                                );
                                            } else {
                                                self.total_chunks *= 2;
                                                debug!(
                                                    "[on_read] Increased total chunks to {}",
                                                    self.total_chunks
                                                );
                                            }
                                            self.chunks_received = 0;
                                            debug!("[on_read] Test duration exceeded or max chunk size reached");
                                            self.phase = TestPhase::GetChunksSendChunksCommand;
                                            poll.registry().reregister(
                                                stream,
                                                self.token,
                                                Interest::WRITABLE,
                                            )?;
                                        }
                                    }
                                }
                            }
                            // Remove processed line from buffer
                            self.time_buffer = self.time_buffer[pos + 1..].to_vec();
                        }
                    }
                    _ => {}
                }
            }
            Ok(0) => {
                debug!("[on_read] Connection closed by peer");
                return Err(
                    io::Error::new(io::ErrorKind::ConnectionAborted, "Connection closed").into(),
                );
            }
            Ok(_) => {
                debug!("[on_read] Read 0 bytes");
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                debug!("[on_read] WouldBlock");
                return Ok(());
            }
            Err(e) => return Err(e.into()),
        }
        Ok(())
    }

    fn on_write(&mut self, stream: &mut TcpStream, poll: &Poll) -> Result<()> {
        debug!("[on_write] Writing to stream");
        match self.phase {
            TestPhase::GetChunksSendChunksCommand => {
                debug!("[on_write] Sending GETCHUNKS command");
                match stream.write(self.get_chunks_command().as_bytes()) {
                    Ok(0) => {
                        debug!("[on_write] Connection closed by peer");
                        return Err(io::Error::new(
                            io::ErrorKind::ConnectionAborted,
                            "Connection closed",
                        )
                        .into());
                    }
                    Ok(_) => {
                        debug!("[on_write] Sent GETCHUNKS command");
                        self.phase = TestPhase::GetChunksReceiveChunk; // Transition to progress phase
                        poll.registry()
                            .reregister(stream, self.token, Interest::READABLE)?;
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                        debug!("[on_write] WouldBlock");
                        return Ok(());
                    }
                    Err(e) => {
                        debug!("[on_write] Error: {}", e);
                        return Err(e.into());
                    }
                }
            }

            TestPhase::GetChunksSendOk => {
                debug!("[on_write] Sending OK command");

                match stream.write(self.get_ok_command().as_bytes()) {
                    Ok(0) => {
                        debug!("[on_write] Connection closed by peer");
                        return Err(io::Error::new(
                            io::ErrorKind::ConnectionAborted,
                            "Connection closed",
                        )
                        .into());
                    }
                    Ok(_) => {
                        debug!("[on_write] Sent OK command");
                        self.phase = TestPhase::GetChunksReceiveTime;
                        poll.registry()
                            .reregister(stream, self.token, Interest::READABLE)?;
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                        debug!("[on_write] WouldBlock");
                        return Ok(());
                    }
                    Err(e) => {
                        debug!("[on_write] Error: {}", e);
                        return Err(e.into());
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn get_phase(&self) -> TestPhase {
        self.phase.clone()
    }
}
