use crate::client::state::TestPhase;
use crate::client::constants::{
    ACCEPT_GETCHUNKS_STRING, MAX_CHUNKS_BEFORE_SIZE_INCREASE, OK_COMMAND,
    PRE_DOWNLOAD_DURATION_NS,
    get_max_chunk_size,
};
use crate::client::state::MeasurementState;
use anyhow::Result;
use log::debug;
use crate::reactor::{Interest, Poll};

pub fn handle_get_chunks_receive_time(
    poll: &Poll,
    state: &mut MeasurementState,
) -> Result<usize, std::io::Error> {
    debug!("handle_get_chunks_receive_time token {:?}", state.token);
    loop {
        let n = state
            .stream
            .read(&mut state.read_buffer[state.read_pos..])?;
        if n == 0 {
            return Ok(0);
        }
        state.read_pos += n;
        let buffer_str = String::from_utf8_lossy(&state.read_buffer[..state.read_pos]);

        if buffer_str.contains(ACCEPT_GETCHUNKS_STRING) {
            if parse_time_response(&buffer_str).is_some() {
                // RMBT spec (phase 2/5): keep doubling the chunk size *while the
                // total pre-test duration has not exceeded d* (PRE_DOWNLOAD_
                // DURATION_NS), NOT while a single operation is under d. Using
                // per-operation time made the pre-test run ~11 round-trips and
                // last up to ~10 s on high-RTT links.
                let pretest_elapsed_ns = state
                    .phase_start_time
                    .map(|t| t.elapsed().as_nanos() as u64)
                    .unwrap_or(u64::MAX);
                if pretest_elapsed_ns < PRE_DOWNLOAD_DURATION_NS
                    && state.chunk_size < get_max_chunk_size() as usize
                {
                    increase_chunk_size(state);
                    state.phase = TestPhase::GetChunksSendChunksCommand;
                    state
                        .stream
                        .reregister(&poll, state.token, Interest::WRITABLE)?;
                    state.read_pos = 0;
                    state.write_pos = 0;
                    return Ok(n);
                } else {
                    state.chunk_size = state.chunk_size as usize;
                    state.phase = TestPhase::GetChunksCompleted;
                    state
                        .stream
                        .reregister(&poll, state.token, Interest::WRITABLE)?;
                    state.read_pos = 0;
                    state.write_pos = 0;
                    return Ok(n);
                }
            }
        }
    }
}

pub fn handle_get_chunks_send_chunks_command(
    poll: &Poll,
    state: &mut MeasurementState,
) -> Result<usize, std::io::Error> {
    debug!(
        "handle_get_chunks_send_chunks_command token {:?}",
        state.token
    );
    let command = format!("GETCHUNKS {} {}\n", state.total_chunks, state.chunk_size);
    if state.write_pos == 0 {
        state.write_buffer[0..command.len()].copy_from_slice(command.as_bytes());
    }
    loop {
        let n = state
            .stream
            .write(&state.write_buffer[state.write_pos..state.write_pos + command.len()])?;
        if n == 0 {
            return Ok(0);
        }
        state.write_pos += n;
        if state.write_pos == command.len() {
            state
                .stream
                .reregister(&poll, state.token, Interest::READABLE)?;
            state.phase = TestPhase::GetChunksReceiveChunk;
            state.chunk_buffer.resize(state.chunk_size as usize, 0);
            state.write_pos = 0;
            state.read_pos = 0;
            return Ok(n);
        }
    }
}

pub fn handle_get_chunks_send_ok(
    poll: &Poll,
    state: &mut MeasurementState,
) -> Result<usize, std::io::Error> {
    debug!("handle_get_chunks_send_ok token {:?}", state.token);
    if state.write_pos == 0 {
        state.write_buffer[0..OK_COMMAND.len()].copy_from_slice(OK_COMMAND);
    }
    loop {
        let n = state
            .stream
            .write(&state.write_buffer[state.write_pos..state.write_pos + OK_COMMAND.len()])?;
        if n == 0 {
            return Ok(0);
        }
        state.write_pos += n;
        if state.write_pos == OK_COMMAND.len() {
            state
                .stream
                .reregister(&poll, state.token, Interest::READABLE)?;
            state.phase = TestPhase::GetChunksReceiveTime;
            state.write_pos = 0;
            return Ok(n);
        }
    }
}

pub fn handle_get_chunks_receive_chunk(
    poll: &Poll,
    state: &mut MeasurementState,
) -> Result<usize, std::io::Error> {
    debug!("handle_get_chunks_receive_chunk token {:?}", state.token);

    loop {
        let n = state
            .stream
            .read(&mut state.chunk_buffer[state.read_pos..])?;
        if n == 0 {
            // EOF: the peer is gone, so the chunk can never complete. Without
            // this the loop would spin on read() forever, burning a core.
            return Ok(0);
        }
        state.read_pos += n;
        if state.read_pos == state.chunk_size as usize {
            if state.chunk_buffer[state.read_pos - 1] == 0x00 {
                state.read_pos = 0;
            } else if state.chunk_buffer[state.read_pos - 1] == 0xFF {
                state.phase = TestPhase::GetChunksSendOk;
                state.read_pos = 0;
                state
                    .stream
                    .reregister(&poll, state.token, Interest::WRITABLE)?;
                return Ok(n);
            }
        }
    }
}

fn parse_time_response(buffer_str: &str) -> Option<u64> {
    buffer_str
        .find("TIME ")
        .and_then(|time_start| {
            buffer_str[time_start..]
                .find('\n')
                .map(|time_end| &buffer_str[time_start + 5..time_start + time_end])
        })
        .and_then(|time_str| time_str.parse::<u64>().ok())
}

fn increase_chunk_size(measurement_state: &mut MeasurementState) {
    if measurement_state.total_chunks < MAX_CHUNKS_BEFORE_SIZE_INCREASE {
        measurement_state.total_chunks *= 2;
    } else {
        measurement_state.chunk_size =
            (measurement_state.chunk_size * 2).min(get_max_chunk_size() as usize);
        measurement_state
            .chunk_buffer
            .resize(measurement_state.chunk_size as usize, 0);
    }
}
