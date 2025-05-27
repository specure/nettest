use anyhow::Result;
use bytes::{Buf, BytesMut};
use log::{debug, info, trace};
use mio::{net::TcpStream, Events, Interest, Poll, Token};
use std::{io, net::SocketAddr, time::{Duration, Instant}};
use std::io::{Read, Write};

use crate::handlers::{GreetingHandler, GetChunksHandler};

const MIN_CHUNK_SIZE: u32 = 4096; // 4KB
const MAX_CHUNK_SIZE: u32 = 4194304; // 4MB
const PRE_DOWNLOAD_DURATION_NS: u64 = 2_000_000_000; // 2 seconds
const MESSAGES_PER_10GB: u32 = 5_000_000;

#[derive(Debug, Clone, PartialEq)]
pub enum TestPhase {
    Greeting,
    TestTokenResponseReceive,
    GetChunksReceive,
    GetChunksProcess,
    PutNoResult,
    Put,
    Get,
    GetNoResult,
    End,
}

pub struct TestState {
    stream: TcpStream,
    poll: Poll,
    events: Events,
    token: Token,
    write_buffer: BytesMut,
    read_buffer: BytesMut,
    chunk_size: Option<u32>,
    test_started: bool,
    bytes_received: u64,
    test_duration: Duration,
    pre_download_end_time: u64,
    pre_download_bytes_read: u64,
    bytes_per_sec_pre_download: Vec<f64>,
    current_chunk_size: u32,
    phase: TestPhase,
    greeting_handler: Option<GreetingHandler>,
    get_chunks_handler: Option<GetChunksHandler>,
    chunks_buffer: BytesMut,
    put_no_result_start_time: Option<Instant>,
}

impl TestState {
    pub fn new(addr: SocketAddr) -> Result<Self> {
        let mut stream = TcpStream::connect(addr)?;
        stream.set_nodelay(true)?;

        let mut poll = Poll::new()?;
        let events = Events::with_capacity(1024);
        let token = Token(0);

        // Register socket for both reading and writing
        poll.registry().register(
            &mut stream,
            token,
            Interest::READABLE | Interest::WRITABLE,
        )?;

        Ok(Self {
            stream,
            poll,
            events,
            token,
            write_buffer: BytesMut::with_capacity(8192),
            read_buffer: BytesMut::with_capacity(8192*4),
            chunk_size: None,
            test_started: false,
            bytes_received: 0,
            test_duration: Duration::from_secs(10),
            pre_download_end_time: 0,
            pre_download_bytes_read: 0,
            bytes_per_sec_pre_download: Vec::new(),
            current_chunk_size: MIN_CHUNK_SIZE,
            phase: TestPhase::Greeting,
            greeting_handler: Some(GreetingHandler::new(token)),
            get_chunks_handler: None,
            chunks_buffer: BytesMut::with_capacity(8192),
            put_no_result_start_time: None,
        })
    }

    pub fn run_measurement(&mut self) -> Result<()> {
        // Send initial upgrade request
        let upgrade_request = "GET /rmbt HTTP/1.1 \r\n\
        Connection: Upgrade \r\n\
        Upgrade: RMBT\r\n\
        RMBT-Version: 1.2.0\r\n\
        \r\n";
        self.write_buffer.extend_from_slice(upgrade_request.as_bytes());
        info!("Prepared upgrade request: {}", upgrade_request);
        debug!("[run_measurement] Initial phase: {:?}", self.phase);

        while self.phase != TestPhase::End {
            self.poll.poll(&mut self.events, None)?;
            let mut needs_write = false;
            let mut needs_read = false;

            trace!("[run_measurement] Events: {:?}", self.events);

            for event in self.events.iter() {
                if event.is_readable() {
                    needs_read = true;
                }
                if event.is_writable() && !self.write_buffer.is_empty() {
                    needs_write = true;
                }
            }

            debug!("[run_measurement] needs_write: {}, needs_read: {}, phase: {:?}", needs_write, needs_read, self.phase);

            if needs_write {
                debug!("[run_measurement] handle_write phase: {:?}", self.phase);
                self.handle_write()?;
            }

            if needs_read {
                debug!("[run_measurement] handle_read phase: {:?}", self.phase);
                self.handle_read()?;
            }
        }

        debug!("[run_measurement] Finished. Final phase: {:?}", self.phase);
        Ok(())
    }

    fn handle_read(&mut self) -> Result<()> {
        let mut buf = match self.phase {
            TestPhase::GetChunksReceive => {
                let chunk_size = self.get_chunks_handler.as_ref().unwrap().chunk_size as usize;
                vec![0u8; chunk_size * 4] // Buffer for 4 chunks of current size
            }
            _ => vec![0u8; 8192] // Default buffer size for other phases
        };

        match self.stream.read(&mut buf) {
            Ok(0) => {
                if self.stream.peer_addr().is_err() {
                    debug!("[handle_read] Connection closed by peer");
                    return Err(io::Error::new(io::ErrorKind::ConnectionAborted, "Connection closed").into());
                }
                return Ok(());
            }
            Ok(n) => {
                let response = String::from_utf8_lossy(&buf[..n]);
                debug!("[handle_read] Phase: {:?}, Bytes: {} (buffer size: {})", 
                    self.phase, n, buf.len());

                match self.phase {
                    TestPhase::Greeting => {
                        debug!("[handle_read] Greeting phase");
                        if let Some(next_phase) = self.greeting_handler.as_mut().unwrap().handle(&response)? {
                            debug!("[handle_read] Greeting -> {:?}", next_phase);
                            self.phase = next_phase.clone();
                            if next_phase == TestPhase::TestTokenResponseReceive {
                                self.write_buffer.extend_from_slice(self.greeting_handler.as_ref().unwrap().get_token_command().as_bytes());
                                self.greeting_handler.as_mut().unwrap().mark_token_sent();
                                self.poll.registry().reregister(
                                    &mut self.stream,
                                    self.token,
                                    Interest::WRITABLE,
                                )?;
                            }
                        }
                        Ok(())
                    }
                    TestPhase::TestTokenResponseReceive => {
                        debug!("[handle_read] TestTokenResponseReceive phase");
                        if response.contains("OK") {
                            debug!("[handle_read] Token accepted, switching to GetChunksReceive");
                            self.phase = TestPhase::GetChunksReceive;
                            self.get_chunks_handler = Some(GetChunksHandler::new());
                            self.write_buffer.extend_from_slice(self.get_chunks_handler.as_ref().unwrap().get_chunks_command().as_bytes());
                            self.poll.registry().reregister(
                                &mut self.stream,
                                self.token,
                                Interest::WRITABLE,
                            )?;
                        }
                        Ok(())
                    }
                    TestPhase::GetChunksReceive => {
                        debug!("[handle_read] GetChunksReceive phase");
                        self.chunks_buffer.extend_from_slice(&buf[..n]);
                        
                        // Check if this is the ACCEPT command
                        if self.chunks_buffer.starts_with(b"ACCEPT") {
                            debug!("[handle_read] Received ACCEPT command");
                            if let Some(pos) = self.chunks_buffer.iter().position(|&b| b == b'\n') {
                                debug!("[handle_read] Found newline at position {}, clearing buffer up to that point", pos);
                                self.chunks_buffer.advance(pos + 1);
                            } else {
                                debug!("[handle_read] No newline found in ACCEPT command, waiting for more data");
                            }
                            return Ok(());
                        }
                        
                        let chunk_size = self.get_chunks_handler.as_ref().unwrap().chunk_size as usize;
                        let total_chunks = self.get_chunks_handler.as_ref().unwrap().total_chunks;
                        
                        // Check if we've reached timeout
                        let test_duration = self.get_chunks_handler.as_ref().unwrap().test_start_time
                            .map(|start| start.elapsed())
                            .unwrap_or(Duration::from_secs(0));
                        
                        if test_duration >= Duration::from_secs(2) {
                            debug!("[handle_read] Timeout reached ({:?}), moving to next phase", test_duration);
                            self.phase = TestPhase::GetChunksProcess;
                            self.write_buffer.extend_from_slice(self.get_chunks_handler.as_ref().unwrap().get_ok_command().as_bytes());
                            self.poll.registry().reregister(
                                &mut self.stream,
                                self.token,
                                Interest::WRITABLE,
                            )?;
                            return Ok(());
                        }
                        
                        // Calculate how many complete chunks we can process
                        let complete_chunks = self.chunks_buffer.len() / chunk_size;
                        debug!("[handle_read] Can process {} complete chunks (buffer size: {}, chunk size: {})", 
                            complete_chunks, self.chunks_buffer.len(), chunk_size);
                        
                        if complete_chunks > 0 {
                            // Process all complete chunks at once
                            self.get_chunks_handler.as_mut().unwrap().chunks_received += complete_chunks;
                            debug!("[handle_read] Processed {} chunks, total received: {}/{}", 
                                complete_chunks,
                                self.get_chunks_handler.as_ref().unwrap().chunks_received,
                                total_chunks);
                            
                            // Advance buffer by the size of complete chunks
                            debug!("[handle_read] Buffer size after processing chunks: {}", self.chunks_buffer.len());
                            
                            // If we received all expected chunks, check the last byte
                            if self.get_chunks_handler.as_ref().unwrap().chunks_received >= total_chunks {
                                debug!("[handle_read] Received all expected chunks, checking termination byte");
                                // Check if we have enough data for the last chunk
                                if self.chunks_buffer.len() >= chunk_size {
                                    let last_byte = self.chunks_buffer[self.chunks_buffer.len() - 1];
                                    debug!("[handle_read] Last byte of buffer: 0x{:02X}", last_byte);
                                    if last_byte == 0xFF {
                                        debug!("[handle_read] Received termination byte, all chunks received");
                                        self.phase = TestPhase::GetChunksProcess;
                                        self.write_buffer.extend_from_slice(self.get_chunks_handler.as_ref().unwrap().get_ok_command().as_bytes());
                                        self.poll.registry().reregister(
                                            &mut self.stream,
                                            self.token,
                                            Interest::WRITABLE,
                                        )?;
                                        // Clear the buffer after processing all chunks
                                        self.chunks_buffer.clear();
                                    }
                                } else {
                                    debug!("[handle_read] Not enough data for final chunk check (have: {}, need: {})", 
                                        self.chunks_buffer.len(), chunk_size);
                                }
                            } else {
                                // Continue reading if we haven't received all chunks
                                self.chunks_buffer.advance(complete_chunks * chunk_size);
                                debug!("[handle_read] Need more chunks, continuing to read");
                                self.poll.registry().reregister(
                                    &mut self.stream,
                                    self.token,
                                    Interest::READABLE,
                                )?;
                            }
                        } else {
                            debug!("[handle_read] Not enough data for complete chunk (have: {}, need: {})", 
                                self.chunks_buffer.len(), chunk_size);
                        }
                        debug!("[handle_read] Buffer size after processing: {}", self.chunks_buffer.len());
                        Ok(())
                    }
                    TestPhase::GetChunksProcess => {
                        debug!("[handle_read] GetChunksProcess phase");
                        debug!("[handle_read] Processing response: {}", response);
                        
                        // Parse TIME response from server
                        if response.starts_with("TIME") {
                            let time_ns: u64 = response
                                .split_whitespace()
                                .nth(1)
                                .ok_or_else(|| anyhow::anyhow!("Invalid TIME response"))?
                                .parse()?;
                            
                            debug!("[handle_read] Received time: {} ns", time_ns);
                            
                            // Check if we're still within pre-test duration (2 seconds)
                            if time_ns < PRE_DOWNLOAD_DURATION_NS {
                                let handler = self.get_chunks_handler.as_ref().unwrap();
                                let current_chunks = handler.chunks_received;
                                let current_chunk_size = handler.chunk_size;
                                
                                // Check if we've reached max chunk size
                                if current_chunk_size >= MAX_CHUNK_SIZE {
                                    debug!("[handle_read] Max chunk size reached ({}), moving to next phase", current_chunk_size);
                                    self.phase = TestPhase::PutNoResult;
                                    self.start_put_no_result_test()?;
                                    return Ok(());
                                }
                                
                                // Calculate next request parameters
                                let (next_chunks, next_chunk_size) = if current_chunks >= 8 {
                                    // If we reached 8 chunks, double the chunk size instead
                                    (8, current_chunk_size * 2)
                                } else {
                                    // Double the number of chunks
                                    (current_chunks * 2, current_chunk_size)
                                };
                                
                                // Ensure chunk size is within bounds
                                let next_chunk_size = next_chunk_size.max(MIN_CHUNK_SIZE).min(MAX_CHUNK_SIZE);
                                
                                debug!("[handle_read] Next request: {} chunks of {} bytes", next_chunks, next_chunk_size);
                                
                                // Update handler parameters
                                let handler = self.get_chunks_handler.as_mut().unwrap();
                                handler.total_chunks = next_chunks;
                                handler.chunk_size = next_chunk_size;
                                handler.chunks_received = 0;
                                
                                // Send next GETCHUNKS request
                                self.write_buffer.extend_from_slice(
                                    handler.get_chunks_command().as_bytes()
                                );
                                self.poll.registry().reregister(
                                    &mut self.stream,
                                    self.token,
                                    Interest::WRITABLE,
                                )?;
                                
                                // Switch back to receiving chunks
                                self.phase = TestPhase::GetChunksReceive;
                            } else {
                                // Pre-test duration exceeded, move to next phase
                                debug!("[handle_read] Pre-test duration exceeded, moving to next phase");
                                self.phase = TestPhase::PutNoResult;
                                self.start_put_no_result_test()?;
                            }
                        }
                        Ok(())
                    }
                    TestPhase::PutNoResult => {
                        debug!("[handle_read] PutNoResult phase");
                        if response.contains("OK") {
                            debug!("[handle_read] Received ACCEPT for PUT_NO_RESULT");
                            // Start the test timer
                            self.put_no_result_start_time = Some(Instant::now());
                            // Start sending data chunks
                            let chunk_size = self.current_chunk_size as usize;
                            let mut data = vec![0u8; chunk_size];
                            // Fill with test data
                            for i in 0..chunk_size - 1 {
                                data[i] = (i % 256) as u8;
                            }
                            // Set terminator byte to 0x00 for intermediate chunks
                            data[chunk_size - 1] = 0x00;
                            // Send data
                            self.write_buffer.extend_from_slice(&data);
                            self.poll.registry().reregister(
                                &mut self.stream,
                                self.token,
                                Interest::WRITABLE,
                            )?;
                        } else if response.contains("TIME") {
                            debug!("[handle_read] Received TIME response for PUT_NO_RESULT");
                            // Parse time and move to next phase
                            let time_ns: u64 = response
                                .split_whitespace()
                                .nth(1)
                                .ok_or_else(|| anyhow::anyhow!("Invalid TIME response"))?
                                .parse()?;
                            debug!("[handle_read] PUT_NO_RESULT completed in {} ns", time_ns);
                            self.phase = TestPhase::Put;
                            let optimal_chunk_size = self.calculate_chunk_size();
                            self.start_put_test(optimal_chunk_size)?;
                        } else if response.contains("OK") {
                            // Server acknowledged the final chunk, wait for TIME response
                            debug!("[handle_read] Server acknowledged final chunk");
                        } else if self.write_buffer.is_empty() {
                            // If we haven't reached 2 seconds, send another chunk
                            if let Some(start_time) = self.put_no_result_start_time {
                                if start_time.elapsed() < Duration::from_secs(2) {
                                    let chunk_size = self.current_chunk_size as usize;
                                    let mut data = vec![0u8; chunk_size];
                                    // Fill with test data
                                    for i in 0..chunk_size - 1 {
                                        data[i] = (i % 256) as u8;
                                    }
                                    // Set terminator byte to 0x00 for intermediate chunks
                                    data[chunk_size - 1] = 0x00;
                                    // Send data
                                    self.write_buffer.extend_from_slice(&data);
                                    self.poll.registry().reregister(
                                        &mut self.stream,
                                        self.token,
                                        Interest::WRITABLE,
                                    )?;
                                } else {
                                    // Send final chunk with 0xFF terminator
                                    let chunk_size = self.current_chunk_size as usize;
                                    let mut data = vec![0u8; chunk_size];
                                    // Fill with test data
                                    for i in 0..chunk_size - 1 {
                                        data[i] = (i % 256) as u8;
                                    }
                                    // Set terminator byte to 0xFF for the final chunk
                                    data[chunk_size - 1] = 0xFF;
                                    // Send data
                                    self.write_buffer.extend_from_slice(&data);
                                    self.poll.registry().reregister(
                                        &mut self.stream,
                                        self.token,
                                        Interest::WRITABLE,
                                    )?;
                                }
                            }
                        }
                        Ok(())
                    }
                    TestPhase::Put => {
                        debug!("[handle_read] PutNoResult phase");
                        if response.contains("OK") {
                            debug!("[handle_read] PutNoResult -> Get");
                            self.phase = TestPhase::Get;
                            self.start_get_test()?;
                        }
                        Ok(())
                    }
                    TestPhase::Get => {
                        debug!("[handle_read] Get phase");
                        if response.contains("OK") {
                            debug!("[handle_read] Get -> GetNoResult");
                            self.phase = TestPhase::GetNoResult;
                            self.start_get_no_result_test()?;
                        }
                        Ok(())
                    }
                    TestPhase::GetNoResult => {
                        debug!("[handle_read] GetNoResult phase");
                        if response.contains("OK") {
                            debug!("[handle_read] GetNoResult -> End");
                            self.phase = TestPhase::End;
                        }
                        Ok(())
                    }
                    TestPhase::End => {
                        debug!("[handle_read] End phase");
                        Ok(())
                    }
                }
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                debug!("[handle_read] WouldBlock");
                Ok(())
            }
            Err(e) => {
                debug!("[handle_read] Error: {}", e);
                Err(e.into())
            }
        }
    }

    fn handle_write(&mut self) -> Result<()> {
        let n = match self.stream.write(&self.write_buffer) {
            Ok(0) => {
                if self.stream.peer_addr().is_err() {
                    info!("Connection closed by peer");
                    debug!("[handle_write] Connection closed by peer");
                    return Err(io::Error::new(io::ErrorKind::ConnectionAborted, "Connection closed").into());
                }
                return Ok(());
            }
            Ok(n) => n,
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                info!("WouldBlock on write");
                debug!("[handle_write] WouldBlock");
                return Ok(());
            }
            Err(e) => {
                info!("Write error: {}", e);
                debug!("[handle_write] Error: {}", e);
                return Err(e.into());
            }
        };

        info!("Sent {} bytes", n);
        debug!("[handle_write] Sent {} bytes, buffer left: {}", n, self.write_buffer.len());
        self.write_buffer.advance(n);

        if self.write_buffer.is_empty() {
            self.poll.registry().reregister(
                &mut self.stream,
                self.token,
                Interest::READABLE,
            )?;
            debug!("[handle_write] Buffer empty, reregistered for READABLE");
        }

        Ok(())
    }

    pub fn get_bytes_received(&self) -> u64 {
        self.bytes_received
    }

    pub fn get_test_duration(&self) -> Duration {
        self.test_duration
    }

    fn start_pre_download(&mut self, start_time: Instant) -> Result<()> {
        self.pre_download_end_time = start_time.elapsed().as_nanos() as u64 + PRE_DOWNLOAD_DURATION_NS;
        self.write_buffer.extend_from_slice(self.get_chunks_handler.as_ref().unwrap().get_chunks_command().as_bytes());
        self.poll.registry().reregister(
            &mut self.stream,
            self.token,
            Interest::WRITABLE,
        )?;
        Ok(())
    }

    fn calculate_chunk_size(&self) -> u32 {
        let bytes_per_sec = self.get_chunks_handler.as_ref().unwrap().get_bytes_per_sec();
        let bytes_per_sec_total: f64 = bytes_per_sec.iter().sum();
        let avg_bytes_per_sec = bytes_per_sec_total / bytes_per_sec.len() as f64;
        
        // Calculate chunk size to get 1 chunk every 20ms on average
        let mut chunk_size = (avg_bytes_per_sec / (1000.0 / 20.0)) as u32;
        
        // Ensure chunk size is within bounds
        chunk_size = chunk_size.max(MIN_CHUNK_SIZE).min(MAX_CHUNK_SIZE);
        
        debug!("Calculated chunk size: {} bytes", chunk_size);
        chunk_size
    }

    fn start_put_test(&mut self, chunk_size: u32) -> Result<()> {
        self.write_buffer.extend_from_slice(format!("PUT {}\n", chunk_size).as_bytes());
        self.poll.registry().reregister(
            &mut self.stream,
            self.token,
            Interest::WRITABLE,
        )?;
        Ok(())
    }

    fn start_put_no_result_test(&mut self) -> Result<()> {
        self.write_buffer.extend_from_slice(format!("PUTNORESULT {}\n", self.current_chunk_size).as_bytes());
        self.poll.registry().reregister(
            &mut self.stream,
            self.token,
            Interest::WRITABLE,
        )?;
        Ok(())
    }

    fn start_get_test(&mut self) -> Result<()> {
        self.write_buffer.extend_from_slice(format!("GET {}\n", self.current_chunk_size).as_bytes());
        self.poll.registry().reregister(
            &mut self.stream,
            self.token,
            Interest::WRITABLE,
        )?;
        Ok(())
    }

    fn start_get_no_result_test(&mut self) -> Result<()> {
        self.write_buffer.extend_from_slice(format!("GET_NO_RESULT {}\n", self.current_chunk_size).as_bytes());
        self.poll.registry().reregister(
            &mut self.stream,
            self.token,
            Interest::WRITABLE,
        )?;
        Ok(())
    }

}
