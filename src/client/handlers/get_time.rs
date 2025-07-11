use anyhow::Result;
use log::{debug};
use mio::{Interest, Poll};
use std::time::Instant;

use crate::client::state::TestPhase;
use crate::client::utils::ACCEPT_GETCHUNKS_STRING;
use crate::client::{ MeasurementState};

const TEST_DURATION_NS: u64 = 7_000_000_000; // 7 seconds
const MAX_CHUNK_SIZE: u32 = 4194304; // 4MB


pub fn handle_get_time_send_ok(
    poll: &Poll,
    state: &mut MeasurementState,
) -> Result<usize, std::io::Error> {
    debug!("handle_get_time_send_ok token {:?}", state.token);
    if state.write_pos == 0 {
        state.write_buffer[0..b"OK\n".len()].copy_from_slice(b"OK\n");
    }
    loop {
        let n = state
            .stream
            .write(&state.write_buffer[state.write_pos..b"OK\n".len()])?;
        state.write_pos += n;
        if state.write_pos == b"OK\n".len() {
            state.write_pos = 0;
            state.read_pos = 0;
            state.phase = TestPhase::GetTimeReceiveTime;
            state
                .stream
                .reregister(&poll, state.token, Interest::READABLE)?;
            return Ok(n);
        }
    }
}

pub fn handle_get_time_send_command(
    poll: &Poll,
    state: &mut MeasurementState,
) -> Result<usize, std::io::Error> {
    debug!("handle_get_time_send_command token {:?}", state.token);

    let command = format!(
        "GETTIME {} {}\n",
        TEST_DURATION_NS / 1_000_000_000,
        state.chunk_size
    );
    if state.write_pos == 0 {
        state.write_buffer[0..command.len()].copy_from_slice(command.as_bytes());
    }

    loop {
        let n = state
            .stream
            .write(&state.write_buffer[state.write_pos..command.len()])?;
        state.write_pos += n;
        if state.write_pos == command.len() {
            state.write_pos = 0;
            state.phase_start_time = Some(Instant::now());

            state.phase = TestPhase::GetTimeReceiveChunk;
            state.chunk_buffer.resize(state.chunk_size, 0);
            state
                .stream
                .reregister(&poll, state.token, Interest::READABLE)?;
            return Ok(n);
        }
    }
}

pub fn handle_get_time_receive_chunk(
    poll: &Poll,
    state: &mut MeasurementState,
) -> Result<usize, std::io::Error> {
    loop {
        debug!(
            "token {:?} started loop read_pos: {}",
            state.token, state.read_pos
        );
        let n = state
            .stream
            .read(&mut state.chunk_buffer[state.read_pos..])?;
        debug!("token {:?} read {} bytes", state.token, n);
        state.read_pos += n;
        state.bytes_received += n as u64;
        if state.read_pos == state.chunk_size {
            state.measurements.push_back((
                state.phase_start_time.unwrap().elapsed().as_nanos() as u64,
                state.bytes_received,
            ));

            if state.chunk_buffer[state.read_pos - 1] == 0xFF {
                state.phase = TestPhase::GetTimeSendOk;
                state
                    .stream
                    .reregister(&poll, state.token, Interest::WRITABLE)?;
                return Ok(n);
            }
            state.read_pos = 0;
        }
    }
}

pub fn handle_get_time_receive_time(
    poll: &Poll,
    state: &mut MeasurementState,
) -> Result<usize, std::io::Error> {
    debug!("handle_get_time_receive_time token {:?}", state.token);
    loop {
        let n = state
            .stream
            .read(&mut state.read_buffer[state.read_pos..])?;
        state.read_pos += n;
        let buffer_str = String::from_utf8_lossy(&state.read_buffer[..state.read_pos]);

        if buffer_str.contains(ACCEPT_GETCHUNKS_STRING) {
            if let Some(time_ns) = buffer_str
                .split_whitespace()
                .nth(1)
                .and_then(|s| s.parse::<u64>().ok())
            {
                let speed = state.bytes_received as f64 / time_ns as f64;
                state.download_speed = Some(speed);
                state.download_time = Some(time_ns);
                state.download_bytes = Some(state.bytes_received);
                state.phase = TestPhase::GetTimeCompleted;
                state.phase_start_time = None;
                state
                    .stream
                    .reregister(&poll, state.token, Interest::WRITABLE)?;
                return Ok(n);
            }
        }
    }
}
