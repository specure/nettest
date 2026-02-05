use anyhow::Result;
use log::debug;
use mio::{Interest, Poll};
use std::time::Instant;

use crate::client::constants::OK_COMMAND;
use crate::client::globals::{CHUNK_STORAGE, CHUNK_TERMINATION_STORAGE};
use crate::client::state::{MeasurementState, TestPhase};

/// Nominal duration of uplink measurement in nanoseconds (7 seconds)
const UPLINK_DURATION_NS: u64 = 7_000_000_000;

pub fn handle_put_send_command(
    poll: &Poll,
    state: &mut MeasurementState,
) -> Result<usize, std::io::Error> {
    debug!("handle_put_send_command token {:?}", state.token);
    let command = format!("PUT {}\n", state.chunk_size);
    if state.write_pos == 0 {
        state.write_buffer[..command.len()].copy_from_slice(command.as_bytes());
    }
    loop {
        let n = state
            .stream
            .write(&state.write_buffer[state.write_pos..command.len()])?;
        state.write_pos += n;
        if state.write_pos == command.len() {
            state.phase = TestPhase::PutReceiveOk;
            state
                .stream
                .reregister(&poll, state.token, Interest::READABLE)?;
            state.write_pos = 0;
            state.read_pos = 0;
            return Ok(n);
        }
    }
}

pub fn handle_put_receive_ok(
    poll: &Poll,
    state: &mut MeasurementState,
) -> Result<usize, std::io::Error> {
    debug!("handle_put_receive_ok token {:?}", state.token);
    loop {
        let n = state
            .stream
            .read(&mut state.read_buffer[state.read_pos..state.read_pos + OK_COMMAND.len()])?;
        state.read_pos += n;
        if state.read_pos >= OK_COMMAND.len() {
            let received = &state.read_buffer[..state.read_pos];
            if received.starts_with(OK_COMMAND) {
                state.phase = TestPhase::PutSendChunks;
                state.phase_start_time = Some(Instant::now());
                state
                    .stream
                    .reregister(&poll, state.token, Interest::WRITABLE)?;
                state.read_pos = 0;
                state.write_pos = 0;
                state.bytes_sent = 0;
                return Ok(n);
            }
        }
    }
}

pub fn handle_put_send_chunks(
    poll: &Poll,
    state: &mut MeasurementState,
) -> Result<usize, std::io::Error> {
    debug!("handle_put_send_chunks token {:?}", state.token);

    if state.phase_start_time.is_none() {
        state.phase_start_time = Some(Instant::now());
    }

    let start_time = state.phase_start_time.unwrap();
    let elapsed_ns = start_time.elapsed().as_nanos() as u64;

    // Check if test duration exceeded
    if elapsed_ns >= UPLINK_DURATION_NS {
        state.phase = TestPhase::PutSendLastChunk;
        state
            .stream
            .reregister(&poll, state.token, Interest::WRITABLE)?;
        state.write_pos = 0;
        return Ok(0);
    }

    // Get chunk buffer
    let buffer = CHUNK_STORAGE.get(&(state.chunk_size as u64)).unwrap();

    loop {
        // Write from current position
        let written = state.stream.write(&buffer[state.write_pos..])?;
        if written == 0 {
            return Ok(0);
        }
        state.bytes_sent += written as u64;
        state.write_pos += written;

        // Check if chunk is complete
        if state.write_pos == state.chunk_size {
            let elapsed = start_time.elapsed().as_nanos() as u64;

            // Check if we should send last chunk
            if elapsed >= UPLINK_DURATION_NS {
                state.phase = TestPhase::PutSendLastChunk;
                state
                    .stream
                    .reregister(&poll, state.token, Interest::WRITABLE)?;
                state.write_pos = 0;
                return Ok(written);
            }

            // Reset for next chunk
            state.write_pos = 0;

            // After sending each chunk, server sends TIME BYTES
            // Switch to readable to receive TIME BYTES from server
            state.phase = TestPhase::PutReceiveTimeBytes;
            state
                .stream
                .reregister(&poll, state.token, Interest::READABLE)?;
            return Ok(written);
        }
    }
}

pub fn handle_put_receive_time_bytes(
    poll: &Poll,
    state: &mut MeasurementState,
) -> Result<usize, std::io::Error> {
    debug!("handle_put_receive_time_bytes token {:?}", state.token);

    loop {
        let n = state
            .stream
            .read(&mut state.read_buffer[state.read_pos..])?;
        state.read_pos += n;

        let buffer_str = String::from_utf8_lossy(&state.read_buffer[..state.read_pos]);

        // Read until we get a complete line (ending with \n)
        if let Some(newline_pos) = buffer_str.find('\n') {
            // Extract the complete message up to and including \n
            let message = &buffer_str[..newline_pos + 1];

            // Parse TIME BYTES response
            if let Some(time_bytes) = parse_time_bytes_response(message) {
                let (time_ns, bytes) = time_bytes;
                debug!(
                    "Received TIME {} BYTES {} token {:?}",
                    time_ns, bytes, state.token
                );

                // Store measurement
                state.upload_measurements.push_back((time_ns, bytes));
            } else {
                debug!(
                    "Failed to parse TIME BYTES from message: {}",
                    message.trim()
                );
            }

            // Clear processed data (move remaining data to start of buffer)
            let remaining = state.read_pos - (newline_pos + 1);
            if remaining > 0 {
                state
                    .read_buffer
                    .copy_within(newline_pos + 1..state.read_pos, 0);
            }
            state.read_pos = remaining;

            // Continue sending chunks
            state.phase = TestPhase::PutSendChunks;
            state
                .stream
                .reregister(&poll, state.token, Interest::WRITABLE)?;
            return Ok(n);
        }
    }
}

pub fn handle_put_send_last_chunk(
    poll: &Poll,
    state: &mut MeasurementState,
) -> Result<usize, std::io::Error> {
    debug!("handle_put_send_last_chunk token {:?}", state.token);

    let buffer = CHUNK_TERMINATION_STORAGE
        .get(&(state.chunk_size as u64))
        .unwrap();

    loop {
        let n = state.stream.write(&buffer[state.write_pos..])?;
        state.bytes_sent += n as u64;
        state.write_pos += n;

        if state.write_pos == state.chunk_size {
            state.phase = TestPhase::PutReceiveFinalTime;
            state
                .stream
                .reregister(&poll, state.token, Interest::READABLE)?;
            state.write_pos = 0;
            state.read_pos = 0;
            return Ok(n);
        }
    }
}

pub fn handle_put_receive_final_time(
    poll: &Poll,
    state: &mut MeasurementState,
) -> Result<usize, std::io::Error> {
    debug!("handle_put_receive_final_time token {:?}", state.token);

    loop {
        let n = state
            .stream
            .read(&mut state.read_buffer[state.read_pos..])?;
        state.read_pos += n;

        // Read until we get a complete line (ending with \n)
        let buffer_str = String::from_utf8_lossy(&state.read_buffer[..state.read_pos]);

        // Check for ACCEPT message first - this indicates the final TIME has been sent
        if buffer_str.contains("ACCEPT GETCHUNKS GETTIME PUT PUTNORESULT PING QUIT\n") {
            // Now parse the final TIME <t> response (server sends TIME <t>\n before ACCEPT)
            if let Some(newline_pos) = buffer_str.find('\n') {
                let message = &buffer_str[..newline_pos + 1];

                if let Some(time_ns) = parse_time_response(message) {
                    debug!("Received final TIME {} token {:?}", time_ns, state.token);

                    // Store final time (bytes were already counted during chunk sending)
                    state.upload_time = Some(time_ns);
                    // state.upload_bytes = Some(state.bytes_sent);
                }
            }

            state.phase = TestPhase::PutCompleted;
            state
                .stream
                .reregister(&poll, state.token, Interest::READABLE)?;
            state.read_pos = 0;
            state.write_pos = 0;
            return Ok(n);
        }

        // Continue reading if ACCEPT not found yet
        if let Some(newline_pos) = buffer_str.find('\n') {
            // Clear processed data (move remaining data to start of buffer)
            let remaining = state.read_pos - (newline_pos + 1);
            if remaining > 0 {
                state
                    .read_buffer
                    .copy_within(newline_pos + 1..state.read_pos, 0);
            }
            state.read_pos = remaining;
        }
    }
}

fn parse_time_response(buffer_str: &str) -> Option<u64> {
    // Look for "TIME <t>" pattern
    if let Some(time_start) = buffer_str.find("TIME ") {
        let time_str_start = time_start + 5;
        let time_end = buffer_str[time_str_start..]
            .find(|c: char| c == '\n' || c == ' ')
            .unwrap_or(buffer_str[time_str_start..].len());

        let time_str = &buffer_str[time_str_start..time_str_start + time_end];

        if let Ok(time) = time_str.parse::<u64>() {
            return Some(time);
        }
    }
    None
}

fn parse_time_bytes_response(buffer_str: &str) -> Option<(u64, u64)> {
    // Look for "TIME <t> BYTES <b>" pattern
    if let Some(time_start) = buffer_str.find("TIME ") {
        if let Some(bytes_start) = buffer_str[time_start..].find(" BYTES ") {
            // Extract time: between "TIME " and " BYTES "
            let time_str_start = time_start + 5;
            let time_str_end = time_start + bytes_start;
            let time_str = &buffer_str[time_str_start..time_str_end].trim();

            // Extract bytes: after " BYTES " until \n or end
            let bytes_str_start = time_start + bytes_start + 7; // "TIME " + time + " BYTES "
            let bytes_str_end = buffer_str[bytes_str_start..]
                .find(|c: char| c == '\n' || c == '\r')
                .unwrap_or(buffer_str[bytes_str_start..].len());
            let bytes_str = &buffer_str[bytes_str_start..bytes_str_start + bytes_str_end].trim();

            if let (Ok(time), Ok(bytes)) = (time_str.parse::<u64>(), bytes_str.parse::<u64>()) {
                return Some((time, bytes));
            }
        }
    }
    None
}
