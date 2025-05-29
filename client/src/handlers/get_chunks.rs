use crate::handlers::BasicHandler;
use crate::state::TestPhase;
use anyhow::Result;
use bytes::{Buf, BytesMut};
use log::debug;
use mio::{net::TcpStream, Interest, Poll, Token};
use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

const MIN_CHUNK_SIZE: u32 = 4096; // 4KB
const MAX_CHUNK_SIZE: u32 = 4194304; // 4MB
const PRE_DOWNLOAD_DURATION_NS: u64 = 1_000_000_000; // 2 seconds

pub struct GetChunksHandler {
    token: Token,
    phase: TestPhase,
    chunk_size: u32,
    total_chunks: u32,
    chunks_received: AtomicU32,
    test_start_time: Option<Instant>,
    unprocessed_bytes: AtomicU32,
    read_buffer: BytesMut, 
    time_buffer: BytesMut, //TODO use ring buffer
    write_buffer: BytesMut,
}

impl GetChunksHandler {
    pub fn new(token: Token) -> Result<Self> {
        Ok(Self {
            token,
            phase: TestPhase::GetChunksReceiveAccept,
            chunk_size: MIN_CHUNK_SIZE,
            total_chunks: 1, // Start with 1 chunk as per specification
            chunks_received: AtomicU32::new(0),
            test_start_time: None,
            unprocessed_bytes: AtomicU32::new(0),
            read_buffer: BytesMut::with_capacity(1024), // Start with 1KB buffer
            time_buffer: BytesMut::with_capacity(1024), // Start with 1KB buffer
            write_buffer: BytesMut::with_capacity(1024), // Start with 1KB buffer
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
        match self.phase {
            TestPhase::GetChunksReceiveAccept => {
                debug!("GetChunksReceiveAccept");
                match stream.read(&mut buf) {
                    Ok(n) if n > 0 => {
                        self.read_buffer.extend_from_slice(&buf[..n]);
                            let line = String::from_utf8_lossy(&self.read_buffer);
                            if line.contains("ACCEPT") {
                                self.phase = TestPhase::GetChunksSendChunksCommand;
                                poll.registry().reregister(
                                    stream,
                                    self.token,
                                    Interest::WRITABLE,
                                )?;
                            }
                            self.read_buffer.clear();
                    }
                    Ok(0) => {
                        debug!("Connection closed by peer");
                        return Err(io::Error::new(
                            io::ErrorKind::ConnectionAborted,
                            "Connection closed",
                        )
                        .into());
                    }
                    Ok(_) => {
                        debug!("Read 0 bytes");
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                        debug!("WouldBlock GetChunksReceiveAccept");
                        return Ok(());
                    }
                    Err(e) => return Err(e.into()),
                }
            }
            TestPhase::GetChunksReceiveChunk => {
                match stream.read(&mut buf) {
                    Ok(n) if n > 0 => {
                        // Add unprocessed bytes from previous read to current buffer
                        let current_unprocessed =
                            self.unprocessed_bytes.fetch_add(n as u32, Ordering::SeqCst);
                        let total_bytes = current_unprocessed + n as u32;

                        // Calculate how many complete chunks we have
                        let complete_chunks = total_bytes / self.chunk_size;
                        self.unprocessed_bytes
                            .store(total_bytes % self.chunk_size, Ordering::SeqCst);

                        // Update chunks received count
                        let current_chunks = self
                            .chunks_received
                            .fetch_add(complete_chunks, Ordering::SeqCst);

                        // Check if we've received all chunks
                        if current_chunks + complete_chunks >= self.total_chunks {
                            // Check if this is the last chunk (has termination byte 0xFF)
                            let is_last_chunk = buf[n - 1] == 0xFF;
                            if is_last_chunk {
                                debug!("Received last chunk {}", current_chunks + complete_chunks);
                                // Send OK command after receiving the last chunk
                                self.phase = TestPhase::GetChunksSendOk;

                                poll.registry().reregister(
                                    stream,
                                    self.token,
                                    Interest::WRITABLE,
                                )?;
                               
                                return Ok(()); // Return immediately after changing phase
                            }
                        }

                        // Register for next read
                        poll.registry()
                            .reregister(stream, self.token, Interest::READABLE)?;
                    }
                    Ok(0) => {
                        debug!("Connection closed by peer");
                        return Err(io::Error::new(
                            io::ErrorKind::ConnectionAborted,
                            "Connection closed",
                        )
                        .into());
                    }
                    Ok(_) => {
                        debug!("Read 0 bytes");
                        return Ok(());
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                        debug!("WouldBlock GetChunksReceiveChunk");
                    }
                    Err(e) => return Err(e.into()),
                }
            }
            TestPhase::GetChunksReceiveTime => {
                match stream.read(&mut buf) {
                    Ok(n) if n > 0 => {
                        self.time_buffer.extend_from_slice(&buf[..n]);

                                            self.phase = TestPhase::PingSendPing;
                                            poll.registry().reregister(
                                                stream,
                                                self.token,
                                                Interest::WRITABLE,
                                            )?;
                        // if let Some(pos) = self.time_buffer.windows(1).position(|w| w == b"\n") {
                        //     let line: std::borrow::Cow<'_, str> = String::from_utf8_lossy(&self.time_buffer[..pos]);
                        //     if line.starts_with("TIME") {
                        //         debug!("Received TIME response");
                        //         if let Some(time_ns) = line
                        //             .split_whitespace()
                        //             .nth(1)
                        //             .and_then(|s| s.parse::<u64>().ok())
                        //         {
                        //             debug!("Time: {} ns", time_ns);
                        //             // Double the number of chunks for next iteration if we haven't exceeded duration
                        //             if let Some(start_time) = self.test_start_time {
                        //                 let elapsed = start_time.elapsed();
                        //                 if elapsed.as_nanos() < PRE_DOWNLOAD_DURATION_NS as u128
                        //                     && self.chunk_size < MAX_CHUNK_SIZE
                        //                 {
                        //                     if self.total_chunks < 8 {
                        //                         // First increase number of chunks until we reach 8
                        //                         self.total_chunks *= 2;
                        //                         debug!(
                        //                             "Increased total chunks to {}",
                        //                             self.total_chunks
                        //                         );
                        //                     } else {
                        //                         // Then increase chunk size
                        //                         self.chunk_size =
                        //                             (self.chunk_size * 2).min(MAX_CHUNK_SIZE);
                        //                         debug!(
                        //                             "Increased chunk size to {}",
                        //                             self.chunk_size
                        //                         );
                        //                     }
                        //                     self.chunks_received.store(0, Ordering::SeqCst);
                        //                     self.unprocessed_bytes.store(0, Ordering::SeqCst);
                        //                     debug!(
                        //                         "Test duration exceeded or max chunk size reached"
                        //                     );
                        //                     self.phase = TestPhase::GetChunksSendChunksCommand;
                        //                     poll.registry().reregister(
                        //                         stream,
                        //                         self.token,
                        //                         Interest::WRITABLE,
                        //                     )?;
                        //                 } else {
                        //                     self.phase = TestPhase::PingSendPing;
                        //                     poll.registry().reregister(
                        //                         stream,
                        //                         self.token,
                        //                         Interest::WRITABLE,
                        //                     )?;
                        //                 }
                        //             }
                        //         }
                        //     }
                        //     self.time_buffer = BytesMut::from(&self.time_buffer[pos + 1..]);
                        // }
                    }
                    Ok(0) => {
                        debug!("Connection closed by peer");
                        return Err(io::Error::new(
                            io::ErrorKind::ConnectionAborted,
                            "Connection closed",
                        )
                        .into());
                    }
                    Ok(_) => {
                        debug!("Read 0 bytes");
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                        debug!("WouldBlock GetChunksReceiveTime {}", e.to_string());
                        poll.registry().reregister(
                            stream,
                            self.token,
                            Interest::READABLE,
                        )?;
                        self.on_read(stream, poll)?;

                        return Ok(());
                    }
                    Err(e) => return Err(e.into()),
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn on_write(&mut self, stream: &mut TcpStream, poll: &Poll) -> Result<()> {
        match self.phase {
            TestPhase::GetChunksSendChunksCommand => {
                debug!("[on_write] Sending GETCHUNKS command");
                let command = self.get_chunks_command();
                self.write_buffer.extend_from_slice(command.as_bytes());
                match stream.write(&self.write_buffer) {
                    Ok(0) => {
                        debug!("Connection closed by peer");
                        return Err(io::Error::new(
                            io::ErrorKind::ConnectionAborted,
                            "Connection closed",
                        )
                        .into());
                    }
                    Ok(n) => {
                        debug!("Sent GETCHUNKS command");
                        self.write_buffer = BytesMut::from(&self.write_buffer[n..]);
                        if self.write_buffer.is_empty() {
                            self.phase = TestPhase::GetChunksReceiveChunk; // Transition to progress phase
                            self.unprocessed_bytes.store(0, Ordering::SeqCst);
                            self.chunks_received.store(0, Ordering::SeqCst);
                            self.test_start_time = Some(Instant::now());
                            poll.registry()
                                .reregister(stream, self.token, Interest::READABLE)?;
                        }
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                        debug!("WouldBlock GetChunksSendChunksCommand");
                        return Ok(());
                    }
                    Err(e) => {
                        debug!("Error: {}", e);
                        return Err(e.into());
                    }
                }
            }
            TestPhase::GetChunksSendOk => {
                if self.write_buffer.is_empty() {
                    self.write_buffer
                        .extend_from_slice(self.get_ok_command().as_bytes());
                }
                match stream.write(&self.write_buffer) {
                    Ok(0) => {
                        debug!("Connection closed by peer");
                        return Err(io::Error::new(
                            io::ErrorKind::ConnectionAborted,
                            "Connection closed",
                        )
                        .into());
                    }
                    Ok(n) => {
                        self.phase = TestPhase::GetChunksReceiveTime;
                        debug!("GetChunksSendOk");
                        self.write_buffer =  BytesMut::with_capacity(1024);
                        if self.write_buffer.is_empty() {
                            poll.registry()
                                .reregister(stream, self.token, Interest::READABLE)?;
                        }
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                        debug!("WouldBlock GetChunksSendOk");
                        return Ok(());
                    }
                    Err(e) => {
                        debug!("Error: {}", e);
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
