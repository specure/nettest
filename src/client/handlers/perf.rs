use anyhow::Result;
use bytes::BytesMut;
use log::{debug, trace};
use mio::{Interest, Poll, Token};
use std::io::{self};
use std::time::Instant;

use crate::client::globals::{CHUNK_STORAGE, CHUNK_TERMINATION_STORAGE};
use crate::client::handlers::BasicHandler;
use crate::client::state::TestPhase;
use crate::client::{write_all_nb, MeasurementState};

const TEST_DURATION_NS: u64 = 7_000_000_000; // 3 seconds
const MAX_CHUNK_SIZE: u64 = 4194304; // 2MB

pub struct PerfHandler {
    token: Token,
    chunk_size: u64,
    test_start_time: Option<Instant>,
    bytes_sent: u64,
    write_buffer: BytesMut,
    read_buffer: BytesMut,
    upload_speed: Option<f64>,
}

impl PerfHandler {
    pub fn new(token: Token) -> Result<Self> {
        Ok(Self {
            token,
            chunk_size: MAX_CHUNK_SIZE,
            test_start_time: None,
            bytes_sent: 0,
            write_buffer: BytesMut::with_capacity(1024),
            read_buffer: BytesMut::with_capacity(1024),
            upload_speed: None,
        })
    }

    pub fn get_put_no_result_command(&self) -> String {
        format!("PUTNORESULT {}\n", self.chunk_size)
    }
}

impl BasicHandler for PerfHandler {
    fn on_read(&mut self, poll: &Poll, measurement_state: &mut MeasurementState) -> Result<()> {
        match measurement_state.phase {
            TestPhase::PerfNoResultReceiveOk => {
                match handle_perf_receive_ok(poll, measurement_state) {
                    Ok(n) => {
                        return Ok(());
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        return Ok(());
                    }
                    Err(e) => {
                        return Err(e.into());
                    }
                }
            }
            TestPhase::PerfNoResultReceiveTime => {
                debug!("PerfNoResultReceiveTime token {:?}", self.token);
                match handle_perf_receive_time(poll, measurement_state) {
                    Ok(n) => {
                        return Ok(());
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        return Ok(());
                    }
                    Err(e) => {
                        return Err(e.into());
                    }
                }
            }
            _ => {
                debug!("PerfNoResult on_read phase: {:?}", measurement_state.phase);
                return Ok(());
            }
        }
    }

    fn on_write(&mut self, poll: &Poll, measurement_state: &mut MeasurementState) -> Result<()> {
        match measurement_state.phase {
            TestPhase::PerfNoResultSendCommand => {
                // debug!("PerfNoResultSendCommand");
                match handle_perf_send_command(poll, measurement_state) {
                    Ok(n) => {
                        return Ok(());
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        return Ok(());
                    }
                    Err(e) => {
                        return Err(e.into());
                    }
                }
            }
            TestPhase::PerfNoResultSendChunks => {
                // debug!("PerfNoResultSendChunks");
                match handle_perf_send_chunks(poll, measurement_state) {
                    Ok(n) => {
                        return Ok(());
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        return Ok(());
                    }
                    Err(e) => {
                        return Err(e.into());
                    }
                }
            }

            TestPhase::PerfNoResultSendLastChunk => {
                debug!("PerfNoResultSendLastChunk");
                match handle_perf_send_last_chunk(poll, measurement_state) {
                    Ok(n) => {
                        return Ok(());
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        return Ok(());
                    }
                    Err(e) => {
                        return Err(e.into());
                    }
                }
            }
            _ => {
                debug!("Perf on write phase: {:?}", measurement_state.phase);
                return self.on_read(poll, measurement_state);
            }
        }
    }
}

pub fn handle_perf_receive_ok(
    poll: &Poll,
    measurement_state: &mut MeasurementState,
) -> Result<usize, std::io::Error> {
    debug!("PerfNoResultReceiveOk");
    loop {
        let n = measurement_state
            .stream
            .read(&mut measurement_state.read_buffer[measurement_state.read_pos..b"OK\n".len()])?;
        if n == b"OK\n".len() {
            measurement_state.phase = TestPhase::PerfNoResultSendChunks;
            measurement_state.stream.reregister(
                &poll,
                measurement_state.token,
                Interest::WRITABLE,
            )?;
            measurement_state.read_pos = 0;
            return Ok(n);
        }
    }
}

pub fn handle_perf_receive_time(
    poll: &Poll,
    measurement_state: &mut MeasurementState,
) -> Result<usize, std::io::Error> {
    debug!("PerfNoResultReceiveTime");
    loop {
        let n = measurement_state
            .stream
            .read(&mut measurement_state.read_buffer[measurement_state.read_pos..])?;
        measurement_state.read_pos += n;

        let line =
            String::from_utf8_lossy(&measurement_state.read_buffer[..measurement_state.read_pos]);
        debug!("Response: {}", line);

        // Ищем строку, которая начинается с TIME и заканчивается \n
        if let Some(time_line) = line.lines().find(|l| l.trim().starts_with("TIME")) {
            let time_ns = time_line
                .split_whitespace()
                .nth(1)
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap();
            let speed_bps =
                calculate_upload_speed(measurement_state.bytes_sent, time_ns);
            measurement_state.upload_speed = Some(speed_bps);
            measurement_state.upload_time = Some(time_ns);
            measurement_state.upload_bytes = Some(measurement_state.bytes_sent);
            measurement_state.phase = TestPhase::PerfNoResultCompleted;
            measurement_state.stream.reregister(
                &poll,
                measurement_state.token,
                Interest::WRITABLE,
            )?;
            measurement_state.read_pos = 0;
            return Ok(n);
        }
    }
}

pub fn handle_perf_send_command(
    poll: &Poll,
    measurement_state: &mut MeasurementState,
) -> Result<usize, std::io::Error> {
    debug!("PerfNoResultSendCommand");

    let command = format!("PUTNORESULT {}\n", measurement_state.chunk_size);
    if measurement_state.write_pos == 0 {
        measurement_state.write_buffer[..command.len()].copy_from_slice(command.as_bytes());
    }
    loop {
        let n = measurement_state
            .stream
            .write(&mut measurement_state.write_buffer[measurement_state.write_pos..command.len()])?;
        measurement_state.write_pos += n;
        if measurement_state.write_pos == command.len() {
            measurement_state.phase = TestPhase::PerfNoResultReceiveOk;
            measurement_state.stream.reregister(
                &poll,
                measurement_state.token,
                Interest::READABLE,
            )?;
            measurement_state.write_pos = 0;
            measurement_state.read_pos = 0;
            return Ok(n);
        }
    }
}

pub fn handle_perf_send_chunks(
    poll: &Poll,
    measurement_state: &mut MeasurementState,
) -> Result<usize, std::io::Error> {
    debug!("PerfNoResultSendChunks");
    if measurement_state.phase_start_time.is_none() {
        measurement_state.phase_start_time = Some(Instant::now());
    }
    if let Some(start_time) = measurement_state.phase_start_time {
        let current_pos = measurement_state.bytes_sent & (measurement_state.chunk_size as u64 - 1);
        let buffer = CHUNK_STORAGE
            .get(&(measurement_state.chunk_size as u64))
            .unwrap();
        let mut remaining: &[u8] = &buffer[current_pos as usize..];
        loop {
            // Write from current position
            let written = measurement_state.stream.write(remaining)?;
            measurement_state.bytes_sent += written as u64;
            remaining = &remaining[written..];
            if remaining.is_empty() {
                let tt = start_time.elapsed().as_nanos();
                let is_last = tt >= TEST_DURATION_NS as u128;
                measurement_state
                    .upload_measurements
                    .push_back((tt as u64, measurement_state.bytes_sent));

                if is_last {
                    debug!("Go to PerfNoResultSendLastChunk");
                    measurement_state.phase = TestPhase::PerfNoResultSendLastChunk;
                    measurement_state.stream.reregister(
                        &poll,
                        measurement_state.token,
                        Interest::WRITABLE,
                    )?;
                    return Ok(written);
                } else {
                    remaining = &buffer;
                }
            }
        }
    } else {
        return Ok(0);
    }
}

pub fn handle_perf_send_last_chunk(
    poll: &Poll,
    measurement_state: &mut MeasurementState,
) -> Result<usize, std::io::Error> {
    debug!("PerfNoResultSendLastChunk");
    let current_pos = measurement_state.bytes_sent & (measurement_state.chunk_size as u64 - 1);
    let buffer = CHUNK_TERMINATION_STORAGE
        .get(&(measurement_state.chunk_size as u64))
        .unwrap();
    let mut remaining = &buffer[current_pos as usize..];

    loop {
        // Write from current position
        let n = measurement_state.stream.write(remaining)?;
        measurement_state.bytes_sent += n as u64;
        remaining = &remaining[n..];
        if remaining.is_empty() {
            measurement_state.phase = TestPhase::PerfNoResultReceiveTime;
            measurement_state.stream.reregister(
                &poll,
                measurement_state.token,
                Interest::READABLE | Interest::WRITABLE,
            )?;
            return Ok(n);
        }
    }
}

pub fn calculate_upload_speed(bytes: u64, time_ns: u64) -> f64 {
    // Convert nanoseconds to seconds, ensuring we don't lose precision
    let time_seconds = if time_ns > u64::MAX / 1_000_000_000 {
        // If time_ns is very large, divide first to avoid overflow
        (time_ns / 1_000_000_000) as f64 + (time_ns % 1_000_000_000) as f64 / 1_000_000_000.0
    } else {
        time_ns as f64 / 1_000_000_000.0
    };
    let speed_bps: f64 = bytes as f64 / time_seconds; // Convert to bytes per second
    debug!("Upload speed calculation:");
    debug!("  Total bytes sent: {}", bytes);
    debug!(
        " Total bytes sent GB: {}",
        bytes as f64 / (1024.0 * 1024.0 * 1024.0)
    );
    debug!("  Total time: {:.9} seconds ({} ns)", time_seconds, time_ns);
    debug!(
        "  Speed: {:.2} MB/s ({:.2} GB/s)  GBit/s: {}  Mbit/s: {}",
        speed_bps / (1024.0 * 1024.0),
        speed_bps / (1024.0 * 1024.0 * 1024.0),
        speed_bps * 8.0 / (1024.0 * 1024.0 * 1024.0),
        speed_bps * 8.0 / (1024.0 * 1024.0)
    );
    speed_bps
}
