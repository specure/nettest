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

#[derive(Default)]
struct PerformanceMetrics {
    encrypted_read_time: Duration,
    decryption_time: Duration,
    decrypted_read_time: Duration,
    raw_tcp_read_time: Duration,
    total_bytes_encrypted: usize,
    total_bytes_decrypted: usize,
    total_bytes_raw: usize,
    read_tls_calls: usize,
    read_tls_total_time: Duration,
    read_tls_total_bytes: usize,
    would_block_time: Duration,
    would_block_count: usize,
    time_between_reads: Duration,
    last_read_time: Option<Instant>,
}

impl PerformanceMetrics {
    fn log_metrics(&self) {
        let encrypted_speed = if !self.encrypted_read_time.is_zero() {
            self.total_bytes_encrypted as f64 / self.encrypted_read_time.as_secs_f64()
        } else {
            0.0
        };
        
        let decrypted_speed = if !self.decrypted_read_time.is_zero() {
            self.total_bytes_decrypted as f64 / self.decrypted_read_time.as_secs_f64()
        } else {
            0.0
        };

        let raw_speed = if !self.raw_tcp_read_time.is_zero() {
            self.total_bytes_raw as f64 / self.raw_tcp_read_time.as_secs_f64()
        } else {
            0.0
        };

        let read_tls_speed = if !self.read_tls_total_time.is_zero() {
            self.read_tls_total_bytes as f64 / self.read_tls_total_time.as_secs_f64()
        } else {
            0.0
        };

        info!("Performance Metrics:");
        info!("  Raw TCP Read: {:.2} MB/s ({} bytes in {:?})", 
            raw_speed / (1024.0 * 1024.0),
            self.total_bytes_raw,
            self.raw_tcp_read_time);
        info!("  Read TLS: {:.2} MB/s ({} bytes in {:?}, {} calls)", 
            read_tls_speed / (1024.0 * 1024.0),
            self.read_tls_total_bytes,
            self.read_tls_total_time,
            self.read_tls_calls);
        info!("  Would Block: {:?} ({} times, avg: {:?})", 
            self.would_block_time,
            self.would_block_count,
            if self.would_block_count > 0 {
                self.would_block_time / self.would_block_count as u32
            } else {
                Duration::from_nanos(0)
            });
        info!("  Time between reads: {:?} (avg: {:?})",
            self.time_between_reads,
            if self.read_tls_calls > 0 {
                self.time_between_reads / self.read_tls_calls as u32
            } else {
                Duration::from_nanos(0)
            });
        info!("  Encrypted Read: {:.2} MB/s ({} bytes in {:?})", 
            encrypted_speed / (1024.0 * 1024.0),
            self.total_bytes_encrypted,
            self.encrypted_read_time);
        info!("  Decryption: {:?} for {} bytes", 
            self.decryption_time,
            self.total_bytes_decrypted);
        info!("  Decrypted Read: {:.2} MB/s ({} bytes in {:?})",
            decrypted_speed / (1024.0 * 1024.0),
            self.total_bytes_decrypted,
            self.decrypted_read_time);
    }
}

pub struct GetTimeHandler {
    token: Token,
    chunk_size: u32,
    bytes_received: u64,
    read_buffer: BytesMut,
    write_buffer: BytesMut,
    responses: Vec<(u64, u64)>, // (time_ns, bytes)
    chunk_buffer: Vec<u8>,      // Буфер для чтения чанков
    metrics: PerformanceMetrics,
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
            metrics: PerformanceMetrics::default(),
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
                match stream {
                    Stream::Tcp(tcp_stream) => {
                        let read_start = Instant::now();
                        match stream.read(&mut self.chunk_buffer) {
                            Ok(n) if n > 0 => {
                                self.bytes_received += n as u64;
                                self.metrics.total_bytes_raw += n;
                                self.metrics.raw_tcp_read_time += read_start.elapsed();
                                
                                if self.chunk_buffer[n - 1] == 0xFF
                                    && (self.bytes_received & (self.chunk_size as u64 - 1)) == 0
                                {
                                    self.metrics.log_metrics();
                                    debug!("Received last chunk");
                                    measurement_state.phase = TestPhase::GetTimeSendOk;
                                    stream.reregister(&poll, self.token, Interest::WRITABLE)?;
                                    return Ok(());
                                }
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
                                return Ok(());
                            }
                            Err(e) => return Err(e.into()),
                        }
                    }
                    Stream::Rustls(tls_stream) => {
                        let mut total_tls_read = 0;
                        let read_start = Instant::now();

                        loop {
                            let read_tls_start = Instant::now();
                            
                            // Измеряем время между вызовами read_tls
                            if let Some(last_read) = self.metrics.last_read_time {
                                self.metrics.time_between_reads += read_tls_start.duration_since(last_read);
                            }
                            self.metrics.last_read_time = Some(read_tls_start);

                            match tls_stream.conn.read_tls(&mut tls_stream.stream) {
                                Ok(0) => {
                                    trace!("TLS stream EOF");
                                    break;
                                }
                                Ok(n) => {
                                    total_tls_read += n;
                                    self.metrics.read_tls_calls += 1;
                                    self.metrics.read_tls_total_bytes += n;
                                    self.metrics.read_tls_total_time += read_tls_start.elapsed();
                                    trace!("Read {} encrypted bytes", n);
                                }
                                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                                    self.metrics.would_block_count += 1;
                                    self.metrics.would_block_time += read_tls_start.elapsed();
                                    trace!(
                                        "Would block on read_tls after {} bytes",
                                        total_tls_read
                                    );
                                    break;
                                }
                                Err(e) => {
                                    match tls_stream.conn.process_new_packets() {
                                        Ok(io_state) => {
                                            let decryption_start = Instant::now();
                                            let mut remaining = io_state.plaintext_bytes_to_read();
                                            self.metrics.decryption_time += decryption_start.elapsed();
                                            trace!("Plaintext bytes available: {}", remaining);
            
                                            let read_start = Instant::now();
                                            while remaining > 0 {
                                                match tls_stream.conn.reader().read(&mut self.chunk_buffer) {
                                                    Ok(0) => break,
                                                    Ok(n) => {
                                                        self.bytes_received += n as u64;
                                                        remaining -= n;
                                                        trace!("Got {} decrypted bytes", n);
            
                                                        if self.chunk_buffer[n - 1] == 0xFF
                                                            && (self.bytes_received
                                                                & (self.chunk_size as u64 - 1))
                                                                == 0
                                                        {
                                                            self.metrics.encrypted_read_time += read_start.elapsed();
                                                            self.metrics.total_bytes_encrypted += total_tls_read;
                                                            self.metrics.total_bytes_decrypted += self.bytes_received as usize;
                                                            self.metrics.log_metrics();
                                                            
                                                            measurement_state.phase = TestPhase::GetTimeSendOk;
                                                            stream.reregister(
                                                                &poll,
                                                                self.token,
                                                                Interest::WRITABLE,
                                                            )?;
                                                            return Ok(());
                                                        }
                                                    }
                                                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                                                        break
                                                    }
                                                    Err(e) => return Err(e.into()),
                                                }
                                            }
                                            self.metrics.decrypted_read_time += read_start.elapsed();
                                        }
                                        Err(e) => return Err(io::Error::new(io::ErrorKind::Other, e).into()),
                                    }
                                },
                            }
                        }

                        self.metrics.encrypted_read_time += read_start.elapsed();
                        self.metrics.total_bytes_encrypted += total_tls_read;

                        // после того как мы прочитали максимум
                        let decryption_start = Instant::now();
                        match tls_stream.conn.process_new_packets() {
                            Ok(io_state) => {
                                self.metrics.decryption_time += decryption_start.elapsed();
                                let mut remaining = io_state.plaintext_bytes_to_read();
                                trace!("Plaintext bytes available: {}", remaining);

                                let read_start = Instant::now();
                                while remaining > 0 {
                                    match tls_stream.conn.reader().read(&mut self.chunk_buffer) {
                                        Ok(0) => break,
                                        Ok(n) => {
                                            self.bytes_received += n as u64;
                                            remaining -= n;
                                            trace!("Got {} decrypted bytes", n);

                                            if self.chunk_buffer[n - 1] == 0xFF
                                                && (self.bytes_received
                                                    & (self.chunk_size as u64 - 1))
                                                    == 0
                                            {
                                                self.metrics.decrypted_read_time += read_start.elapsed();
                                                self.metrics.total_bytes_decrypted += self.bytes_received as usize;
                                                self.metrics.log_metrics();
                                                
                                                measurement_state.phase = TestPhase::GetTimeSendOk;
                                                stream.reregister(
                                                    &poll,
                                                    self.token,
                                                    Interest::WRITABLE,
                                                )?;
                                                return Ok(());
                                            }
                                        }
                                        Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                                            break
                                        }
                                        Err(e) => return Err(e.into()),
                                    }
                                }
                                self.metrics.decrypted_read_time += read_start.elapsed();
                            }
                            Err(e) => return Err(io::Error::new(io::ErrorKind::Other, e).into()),
                        }
                    } // _ => {}

                Stream::OpenSsl(open_ssl_stream) => {
                    let mut total_tls_read = 0;
                    let read_start = Instant::now();
                    return Ok(());
                }
                }
            },
            TestPhase::GetTimeReceiveTime => {
                if read_until(stream, &mut self.read_buffer, ACCEPT_GETCHUNKS_STRING)? {
                    debug!("Received time");
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
                debug!("Sending command to get time");
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
                debug!("Sending OK command");
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
