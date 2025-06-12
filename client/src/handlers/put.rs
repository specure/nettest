use crate::handlers::BasicHandler;
use crate::state::{MeasurementState, TestPhase};
use crate::stream::Stream;
use crate::utils::utils::write_all_nb_loop;
use crate::{read_until, write_all_nb};
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
    read_buffer_temp: Vec<u8>,
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
            read_buffer_temp: vec![0u8; 1024 * 1024 * 10],
            test_start_time: None,
            bytes_sent: 0,
            write_buffer: BytesMut::with_capacity(MAX_CHUNK_SIZE as usize),
            read_buffer: BytesMut::with_capacity(1024 * 1024 * 10),
            data_buffer,
            data_termination_buffer,
            responses: Vec::new(),
        })
    }

    fn calculate_upload_speed(&self) {
        // 1. Собираем все (t, b)
        let mut results = Vec::new();
        for line in self.read_buffer_temp.split(|&b| b == b'\n') {
            if line.is_empty() { continue; }
            let s = String::from_utf8_lossy(line);
            let parts: Vec<&str> = s.split_whitespace().collect();
            if parts.len() >= 4 && parts[0] == "TIME" && parts[2] == "BYTES" {
                if let (Ok(time_ns), Ok(bytes)) = (parts[1].parse::<u64>(), parts[3].parse::<u64>()) {
                    results.push((time_ns, bytes));
                }
            }
        }
        if results.len() < 2 {
            info!("Недостаточно данных для расчета скорости");
            return;
        }

        // 2. t* — последний замер времени
        let t_star = results.last().unwrap().0;

        // 3. Ищем l-1 и l такие, что t_{l-1} < t* <= t_l
        let mut l0 = 0;
        for i in 1..results.len() {
            if results[i].0 >= t_star {
                l0 = i - 1;
                break;
            }
        }
        let (t_lm1, b_lm1) = results[l0];
        let (t_l, b_l) = results[l0 + 1];

        // 4. Интерполяция
        let b_star = b_lm1 as f64
            + (t_star as f64 - t_lm1 as f64) / (t_l as f64 - t_lm1 as f64) * (b_l as f64 - b_lm1 as f64);

        // 5. Скорость
        let speed_bps = b_star * 8.0 / (t_star as f64 / 1_000_000_000.0);
        let speed_gbps = speed_bps / 1_000_000_000.0;
        info!("Uplink speed (interpolated): {:.2} Gbit/s ({:.2} bps)", speed_gbps, speed_bps);
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
            TestPhase::PutSendChunks => {
                debug!(
                    "PutReceiveBytesTime {} ",
                    String::from_utf8_lossy(&self.read_buffer)
                );
                // let mut a = read_until(stream, &mut self.read_buffer, "\n")?;
                // while !a {
                //     a = read_until(stream, &mut self.read_buffer, "\n")?;
                // }
                // if let Some(line) = String::from_utf8_lossy(&self.read_buffer)
                //     .split_whitespace()
                //     .collect::<Vec<_>>()
                //     .get(..4)
                // {
                //     if let (Some(time_ns), Some(bytes)) = (
                //         line.get(1).and_then(|s| s.parse::<u64>().ok()),
                //         line.get(3).and_then(|s| s.parse::<u64>().ok()),
                //     ) {
                //         self.responses.push((time_ns, bytes));
                //         measurement_state.phase = TestPhase::PutSendChunks;
                //         stream.reregister(&poll, self.token, Interest::WRITABLE)?;
                //         self.read_buffer.clear();
                //     }
                // }
                match stream.read(&mut self.read_buffer_temp) {
                    Ok(n) => {
                        debug!("Read {} bytes", n);
                        measurement_state.phase = TestPhase::PutSendChunks;
                        stream.reregister(&poll, self.token, Interest::WRITABLE)?;
                        return Ok(());
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        debug!("WouldBlock PutReceiveBytesTime nothing to read");
                        measurement_state.phase = TestPhase::PutSendChunks;
                        stream.reregister(&poll, self.token, Interest::WRITABLE)?;
                        return Ok(());
                    }
                    Err(e) => {
                        debug!("Error reading from stream: {}", e);
                        return Err(e.into());
                        // error!("Error reading from stream: {}", e);
                    }
                }
            }
            TestPhase::PutReceiveTime => {
                // if let Some(line) = String::from_utf8_lossy(&self.read_buffer)
                //     .split_whitespace()
                //     .collect::<Vec<_>>()
                //     .get(..4)
                // {
                //     if let (Some(time_ns), Some(bytes)) = (
                //         line.get(1).and_then(|s| s.parse::<u64>().ok()),
                //         line.get(3).and_then(|s| s.parse::<u64>().ok()),
                //     ) {
                //         self.responses.push((time_ns, bytes));
                //         measurement_state.phase = TestPhase::PutSendChunks;
                //         stream.reregister(&poll, self.token, Interest::WRITABLE)?;
                //         self.read_buffer.clear();
                //     }
                // }


                self.calculate_upload_speed();

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
                    let mut is_last = elapsed.as_nanos() >= TEST_DURATION_NS as u128;

                    if self.write_buffer.is_empty() {
                        let buffer = if is_last {
                            &self.data_termination_buffer
                        } else {
                            &self.data_buffer
                        };
                        self.write_buffer.extend_from_slice(buffer);
                    }
                    if !is_last {
                        loop {
                            match stream.write(&mut self.write_buffer) {
                                Ok(n) => {
                                    self.write_buffer.advance(n);
                                    if self.write_buffer.is_empty() {
                                        return Ok(());
                                    }
                                }
                                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                                    debug!("WouldBlock PutSendChunks");
                                    // measurement_state.phase = TestPhase::PutReceiveBytesTime;
                                    stream.reregister(&poll, self.token, Interest::READABLE | Interest::WRITABLE)?;
                                    return Ok(());
                                }
                                Err(e) => {
                                    debug!("Error writing to stream: {}", e);
                                }
                            }
                        }

                        // if write_all_nb_loop(&mut self.write_buffer, stream)? {
                        //     measurement_state.phase = TestPhase::PutReceiveBytesTime;
                        //     stream.reregister(&poll, self.token, Interest::READABLE)?;
                        //     return Ok(());
                        // }
                    } else {
                        debug!("Write all nb loop");
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
