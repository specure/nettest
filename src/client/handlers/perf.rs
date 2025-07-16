use anyhow::Result;
use log::{debug, info};
use mio::{Interest, Poll};
use std::time::Instant;

use crate::client::globals::{CHUNK_STORAGE, CHUNK_TERMINATION_STORAGE};
use crate::client::state::{MeasurementState, TestPhase};

const TEST_DURATION_NS: u64 = 7; 

pub fn handle_perf_receive_ok(
    poll: &Poll,
    measurement_state: &mut MeasurementState,
) -> Result<usize, std::io::Error> {
    debug!("handle_perf_receive_ok token {:?}", measurement_state.token);
    loop {
        let n = measurement_state
            .stream
            .read(&mut measurement_state.read_buffer[measurement_state.read_pos..b"OK\n".len()])?;
        if n == b"OK\n".len() {
            measurement_state.phase = TestPhase::PerfSendChunks;
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
    loop {
        let n = measurement_state
            .stream
            .read(&mut measurement_state.read_buffer[measurement_state.read_pos..])?;
        measurement_state.read_pos += n;

        let line =
            String::from_utf8_lossy(&measurement_state.read_buffer[..measurement_state.read_pos]);
        if let Some(time_line) = line.lines().find(|l| l.trim().starts_with("TIME")) {
            let time_ns = time_line
                .split_whitespace()
                .nth(1)
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap();
            // let speed_bps =
            //     calculate_upload_speed(measurement_state.bytes_sent, time_ns);
            // measurement_state.upload_speed = Some(calculate_upload_speed(measurement_state.bytes_sent, time_ns));
            measurement_state.upload_time = Some(time_ns);
            measurement_state.upload_bytes = Some(measurement_state.bytes_sent);
            measurement_state.phase = TestPhase::PerfCompleted;
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
            measurement_state.phase = TestPhase::PerfReceiveOk;
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
    // debug!("handle_perf_send_chunks token {:?}", measurement_state.token);
    if measurement_state.phase_start_time.is_none() {
        measurement_state.write_pos = 0;
        measurement_state.phase_start_time = Some(Instant::now());
    }
    if let Some(start_time) = measurement_state.phase_start_time {
        let buffer = CHUNK_STORAGE
            .get(&(measurement_state.chunk_size as u64))
            .unwrap();
        loop {
            // Write from current position
            let written = measurement_state.stream.write( &buffer[measurement_state.write_pos..])?;
            if written == 0 {
                info!("No data to write");
                return Ok(0);
            }
            measurement_state.bytes_sent += written as u64;
            measurement_state.write_pos += written;

            // debug!("Sent {} bytes token {:?}", measurement_state.bytes_sent, measurement_state.token);
            if measurement_state.write_pos == measurement_state.chunk_size  {
                let tt = start_time.elapsed().as_nanos();
                let is_last = tt >= TEST_DURATION_NS as u128;
                measurement_state
                    .upload_measurements
                    .push_back((tt as u64, measurement_state.bytes_sent));

                if is_last {
                    measurement_state.phase = TestPhase::PerfSendLastChunk;
                    measurement_state.stream.reregister(
                        &poll,
                        measurement_state.token,
                        Interest::WRITABLE,
                    )?;
                    measurement_state.write_pos = 0;
                    return Ok(written);
                } else {
                    measurement_state.write_pos = 0;
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
    // debug!("handle_perf_send_last_chunk token {:?}", measurement_state.token);
    let buffer = CHUNK_TERMINATION_STORAGE
        .get(&(measurement_state.chunk_size as u64))
        .unwrap();

    loop {
        // Write from current position
        let n = measurement_state.stream.write(&buffer[measurement_state.write_pos..])?;
        measurement_state.bytes_sent += n as u64;
        measurement_state.write_pos += n;
        if measurement_state.write_pos == measurement_state.chunk_size {
            measurement_state.phase = TestPhase::PerfReceiveTime;
            measurement_state.stream.reregister(
                &poll,
                measurement_state.token,
                Interest::READABLE,
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
