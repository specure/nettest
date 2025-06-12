use crate::handlers::BasicHandler;
use crate::state::{MeasurementState, TestPhase};
use crate::stream::Stream;
use crate::{read_until, write_all_nb};
use anyhow::Result;
use bytes::{Buf, BytesMut};
use log::debug;
use mio::{net::TcpStream, Interest, Poll, Token};
use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const MIN_PINGS: u32 = 10;
const MAX_PINGS: u32 = 200;
const PING_DURATION_NS: u64 = 1_000_000_000; // 1 second
const PONG_RESPONSE: &[u8] = b"PONG\n";

pub struct PingHandler {
    token: Token,
    test_start_time: Option<Instant>,
    write_buffer: BytesMut,
    time_result: Option<u64>,
    ping_times: Vec<u64>,  // Store all ping times for median calculation
    read_buffer: BytesMut, // Buffer for TIME response
}

impl PingHandler {
    pub fn new(token: Token) -> Result<Self> {
        Ok(Self {
            token,
            test_start_time: None,
            write_buffer: BytesMut::with_capacity(1024),
            time_result: None,
            ping_times: Vec::with_capacity(MAX_PINGS as usize),
            read_buffer: BytesMut::with_capacity(1024),
        })
    }

    pub fn get_ping_command(&self) -> String {
        "PING\n".to_string()
    }

    pub fn get_ok_command(&self) -> String {
        "OK\n".to_string()
    }

    pub fn get_median_latency(&self) -> Option<u64> {
        let mut sorted_times = self.ping_times.clone();
        sorted_times.sort_unstable();

        let mid = sorted_times.len() / 2;
        if sorted_times.len() % 2 == 0 {
            Some((sorted_times[mid - 1] + sorted_times[mid]) / 2)
        } else {
            Some(sorted_times[mid])
        }
    }
}

impl BasicHandler for PingHandler {
    fn on_read(
        &mut self,
        stream: &mut Stream,
        poll: &Poll,
        measurement_state: &mut MeasurementState,
    ) -> Result<()> {
        match measurement_state.phase {
            TestPhase::PingReceivePong => {
                let mut buf1 = vec![0u8; PONG_RESPONSE.len()];
                match stream.read(&mut buf1) {
                    Ok(n) if n > 0 => {
                        measurement_state.phase = TestPhase::PingSendOk;
                        stream.reregister(&poll, self.token, Interest::WRITABLE)?;
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
                        debug!("WouldBlock PingReceivePong");
                        return Ok(());
                    }
                    Err(e) => return Err(e.into()),
                }
            }
            TestPhase::PingReceiveTime => {
                if read_until(
                    stream,
                    &mut self.read_buffer,
                    "ACCEPT GETCHUNKS GETTIME PUT PUTNORESULT PING QUIT\n",
                )? {
                    let elapsed = self.test_start_time.unwrap().elapsed();
                    let buffer_str = String::from_utf8_lossy(&self.read_buffer);
                    if let Some(time_start) = buffer_str.find("TIME ") {
                        if let Some(time_end) = buffer_str[time_start..].find('\n') {
                            let time_str = &buffer_str[time_start + 5..time_start + time_end];

                            if let Ok(time_ns) = time_str.parse::<u64>() {
                                self.ping_times.push(time_ns);
                                let pings_sent = self.ping_times.len();

                                if elapsed.as_nanos() < PING_DURATION_NS as u128
                                    && pings_sent < MAX_PINGS as usize
                                {
                                    measurement_state.phase = TestPhase::PingSendPing;
                                    stream.reregister(&poll, self.token, Interest::WRITABLE)?;
                                    self.read_buffer.clear();
                                } else {
                                    // Calculate final median latency
                                    if let Some(median) = self.get_median_latency() {
                                        debug!("Final median latency: {} ns", median);
                                        self.time_result = Some(median);
                                        measurement_state.ping_median = Some(median);
                                    }
                                    measurement_state.phase = TestPhase::PingCompleted;
                                    stream.reregister(&poll, self.token, Interest::WRITABLE)?;
                                    self.read_buffer.clear();
                                }
                            }
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
            TestPhase::PingSendPing => {
                if self.write_buffer.is_empty() {
                    self.write_buffer
                        .extend_from_slice(self.get_ping_command().as_bytes());
                }
                if write_all_nb(&mut self.write_buffer, stream)? {
                    if self.test_start_time.is_none() {
                        self.test_start_time = Some(Instant::now());
                    }
                    measurement_state.phase = TestPhase::PingReceivePong;
                    stream.reregister(&poll, self.token, Interest::READABLE)?;
                    self.write_buffer.clear();
                }
            }
            TestPhase::PingSendOk => {
                if self.write_buffer.is_empty() {
                    self.write_buffer
                        .extend_from_slice(self.get_ok_command().as_bytes());
                }
                if write_all_nb(&mut self.write_buffer, stream)? {
                    measurement_state.phase = TestPhase::PingReceiveTime;
                    stream.reregister(&poll, self.token, Interest::READABLE)?;
                    self.write_buffer.clear();
                }
            }
            _ => {}
        }
        Ok(())
    }
}
