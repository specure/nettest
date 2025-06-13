use crate::handlers::BasicHandler;
use crate::state::{MeasurementState, TestPhase};
use crate::stream::Stream;
use crate::utils::utils::write_all_nb_loop;
use crate::utils::ACCEPT_GETCHUNKS_STRING;
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
                                     // const BUFFER_SIZE: usize = 4 * 1024 * 1024; // 4mb

pub struct PutHandler {
    token: Token,
    chunk_size: u64,
    test_start_time: Option<Instant>,
    bytes_sent: u64,
    write_buffer: BytesMut,
    read_buffer: BytesMut,
    read_buffer_temp: Vec<u8>,
    read_buffer_temp_accumulated: Vec<u8>,
    data_buffer: Vec<u8>,
    data_termination_buffer: Vec<u8>,
    responses: Vec<(u64, u64)>, // (time_ns, bytes)
    read_times: Vec<Duration>,
    write_times: Vec<Duration>,
    is_last: bool,
    send_data: bool,
}

impl PutHandler {
    pub fn new(token: Token) -> Result<Self> {
        // Create and fill buffer with random data
        let mut data_buffer = vec![0u8; MAX_CHUNK_SIZE as usize];
        for byte in &mut data_buffer {
            *byte = fastrand::u8(..);
        }
        data_buffer[MAX_CHUNK_SIZE as usize - 1] = 0x00;
        let mut data_termination_buffer = vec![0u8; MAX_CHUNK_SIZE as usize];
        for byte in &mut data_termination_buffer {
            *byte = fastrand::u8(..);
        }
        data_termination_buffer[MAX_CHUNK_SIZE as usize - 1] = 0xFF;

        Ok(Self {
            token,
            chunk_size: MAX_CHUNK_SIZE,
            test_start_time: None,
            bytes_sent: 0,
            write_buffer: BytesMut::with_capacity(MAX_CHUNK_SIZE as usize),
            read_buffer: BytesMut::with_capacity(1024 * 1024 * 20),
            read_buffer_temp: vec![0u8; 1024 * 1024 * 10],
            data_buffer,
            data_termination_buffer,
            responses: Vec::new(),
            read_times: Vec::new(),
            write_times: Vec::new(),
            read_buffer_temp_accumulated: vec![0u8; 1024 * 1024],
            is_last: false,
            send_data: false,
        })
    }

    pub fn calculate_upload_speed(&self) {
        let mut results: Vec<(u64, u64)> = Vec::new();

        // Обрабатываем данные из read_buffer_temp
        let buffer_str = String::from_utf8_lossy(&self.read_buffer_temp);
        // debug!("Buffer: {}", buffer_str);
        for line in buffer_str.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 4 {
                if let (Ok(time_ns), Ok(bytes)) = (parts[1].parse::<u64>(), parts[3].parse::<u64>())
                {
                    results.push((time_ns, bytes));
                }
            }
        }

        // Сортируем результаты по времени
        results.sort_by_key(|&(time, _)| time);

        // Находим последнее измерение
        if let Some((t_star, _)) = results.last() {
            // Выполняем интерполяцию
            let mut total_bytes = 0.0;
            for i in 0..results.len() - 1 {
                let (t1, b1) = results[i];
                let (t2, b2) = results[i + 1];
                if t2 >= *t_star {
                    let ratio = (*t_star - t1) as f64 / (t2 - t1) as f64;
                    total_bytes += b1 as f64 + ratio * (b2 - b1) as f64;
                    break;
                }
            }

            // Рассчитываем скорость
            let speed_bps = total_bytes * 8.0 / (*t_star as f64 / 1_000_000_000.0);
            let speed_gbps = speed_bps / 1_000_000_000.0;
            info!(
                "Uplink speed (interpolated): {:.2} Gbit/s ({:.2} bps)",
                speed_gbps, speed_bps
            );
        } else {
            info!("Не удалось рассчитать скорость");
        }
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
                //TODO improve
                if read_until(stream, &mut self.read_buffer, "OK\n")? {
                    measurement_state.phase = TestPhase::PutSendChunks;
                    self.read_buffer.clear();
                    stream.reregister(&poll, self.token, Interest::WRITABLE)?;
                }
            }
            TestPhase::PutSendChunks => {
                debug!("PutSendChunks !!!!!!!!!!!!!! {:?}", self.token);
                let mut temp_accumulated: Vec<u8> = vec![0u8; 1024 * 1024 * 10];
                loop {
                    match stream.read(&mut temp_accumulated) {
                        Ok(n) => {
                            debug!("Read  PutSendChunks {} bytes from stream", n);
                            self.read_buffer_temp
                                .extend_from_slice(&temp_accumulated[..n]);
                            if (String::from_utf8_lossy(&self.read_buffer_temp)
                                .contains(ACCEPT_GETCHUNKS_STRING))
                            {
                                debug!("Accept GETCHUNKS_STRING 112312313412341234123412342342341234123412 {:?}", self.token);
                                //remove 2 last lines till \n from read_buffer_temp
                                self.calculate_upload_speed();
                                measurement_state.read_buffer_temp = self.read_buffer_temp.clone();
                                self.read_buffer_temp.clear();
                                measurement_state.phase = TestPhase::PutCompleted;
                                return Ok(());
                            }
                        }
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                            debug!("Read  PutSendChunks WouldBlock bytes from stream");

                            return self.on_write(stream, poll, measurement_state);
                        }
                        Err(e) => {
                            debug!("Error reading from stream: {}", e);
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
        trace!(
            "PutHandler phase write  {:?} token {:?}",
            measurement_state.phase, self.token
        );
        match measurement_state.phase {
            TestPhase::PutSendCommand => {
                self.write_buffer
                    .extend_from_slice(self.get_put_command().as_bytes());

                if write_all_nb(&mut self.write_buffer, stream)? {
                    measurement_state.phase = TestPhase::PutReceiveOk;
                    stream.reregister(&poll, self.token, Interest::READABLE)?;
                    self.write_buffer.clear();
                    return Ok(());
                }
            }
            TestPhase::PutSendChunks => {
                if self.test_start_time.is_none() {
                    self.test_start_time = Some(Instant::now());
                }
                trace!("Write buffer: {}", self.write_buffer.len());

                if let Some(start_time) = self.test_start_time {
                    let elapsed = start_time.elapsed();
                    self.is_last = elapsed.as_nanos() >= TEST_DURATION_NS as u128;

                    if (!self.is_last || !self.write_buffer.is_empty()) {
                        if self.write_buffer.is_empty() {
                            // let buffer = if self.is_last {
                            //     &self.data_termination_buffer
                            // } else {
                            //     &self.data_buffer
                            // };
                            self.write_buffer.extend_from_slice( &self.data_buffer);
                        }
                        loop {
                            // debug!("Write buffer: {}", self.write_buffer.len());
                            // let write_start = Instant::now();
                            match stream.write(&mut self.write_buffer) {
                                Ok(n) => {
                                    // let write_duration = write_start.elapsed();
                                    // self.write_times.push(write_duration);
                                    // debug!("Wrote {} bytes in {:?}", n, write_duration);
                                    self.write_buffer.advance(n);
                                    if self.write_buffer.is_empty() {
                                        trace!("Write buffer is empty");

                                        stream.reregister(
                                            &poll,
                                            self.token,
                                            Interest::READABLE | Interest::WRITABLE,
                                        )?;
                                        return Ok(());
                                    }
                                }
                                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                                    // debug!("WouldBlock PutSendChunks");
                                    // stream.reregister(
                                    //     &poll,
                                    //     self.token,
                                    //     Interest::READABLE | Interest::WRITABLE,
                                    // )?;
                                    return Ok(());
                                }
                                Err(e) => {
                                    debug!("Error writing to stream: {}", e);
                                }
                            }
                        }
                    } else {
                        debug!("Write send_data: {}", self.send_data);
                        if !self.send_data {
                            if self.write_buffer.is_empty() {
                                self.write_buffer
                                    .extend_from_slice(&self.data_termination_buffer);
                            }
                            loop {
                                debug!("Write loop: {} token {:?}", self.write_buffer.len(), self.token);
                                debug!("Write buffer last byte: {:?}", self.write_buffer.last());
                                match stream.write(&mut self.write_buffer) {
                                    Ok(n) => {
                                        self.write_buffer.advance(n);
                                        debug!("Write buffer after advance: {} token {:?}", self.write_buffer.len(), self.token);
                                        if self.write_buffer.is_empty() { 
                                            debug!("Write buffer is EMTPY EMTPYE MTPYEMTPYEMTP YEMTPYEMTP YEMTPYEMTPYEM EMTPYEMTPYTPY token {:?}", self.token);
                                            self.send_data = true;
                                            stream.reregister(
                                                &poll,
                                                self.token,
                                                Interest::READABLE,
                                            )?;
                                            return Ok(());
                                        }
                                    }
                                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                                        return Ok(());
                                    }
                                    Err(e) => {
                                        debug!("Error writing to stream: {}", e);
                                        return Err(e.into());
                                    }
                                }
                            }
                        } else {
                            debug!("Write send_data: {}", self.send_data);
                            stream.reregister(&poll, self.token, Interest::READABLE)?;
                        }
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }
}
