use anyhow::Result;
use log::{debug, info, trace};
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
    debug!("handle_put_send_command token {:?} chunk_size: {}", state.token, state.chunk_size);
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
                debug!("PUT test started, phase_start_time set, target duration: {} ns ({} seconds)", UPLINK_DURATION_NS, UPLINK_DURATION_NS as f64 / 1_000_000_000.0);
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
    trace!("handle_put_send_chunks token {:?}", state.token);
    
    if state.phase_start_time.is_none() {
        state.write_pos = 0;
        state.phase_start_time = Some(Instant::now());
    }
    
    if let Some(start_time) = state.phase_start_time {
        let buffer = CHUNK_STORAGE
            .get(&(state.chunk_size as u64))
            .unwrap();
        loop {
            // Check time before writing to determine if this should be the last chunk
            let elapsed_ns = start_time.elapsed().as_nanos();
            let is_last = elapsed_ns >= UPLINK_DURATION_NS as u128;
            
            if is_last && state.write_pos == 0 {
                // Time limit reached before starting to write this chunk, switch to sending last chunk
                state.phase = TestPhase::PutSendLastChunk;
                state.stream.reregister(
                    &poll,
                    state.token,
                    Interest::WRITABLE,
                )?;
                debug!("Time limit reached before chunk start ({} ns >= {} ns), switching to last chunk", elapsed_ns, UPLINK_DURATION_NS);
                // Return Ok(1) to indicate successful phase switch (not Ok(0) which would be treated as error)
                return Ok(1);
            }
            
            // Write from current position
            let written = state.stream.write(&buffer[state.write_pos..])?;
            if written == 0 {
                info!("No data to write");
                return Ok(0);
            }
            state.bytes_sent += written as u64;
            state.write_pos += written;

            if state.write_pos == state.chunk_size {
                // Chunk completed, check time again
                let tt = start_time.elapsed().as_nanos();
                let is_last_after_chunk = tt >= UPLINK_DURATION_NS as u128;

                debug!("Chunk completed: elapsed={} ns ({} s), target={} ns ({} s), is_last={}", 
                    tt, tt as f64 / 1_000_000_000.0, 
                    UPLINK_DURATION_NS, UPLINK_DURATION_NS as f64 / 1_000_000_000.0,
                    is_last_after_chunk);

                if is_last_after_chunk {
                    state.phase = TestPhase::PutSendLastChunk;
                    state.stream.reregister(
                        &poll,
                        state.token,
                        Interest::WRITABLE,
                    )?;
                    state.write_pos = 0;
                    debug!("Time limit reached after chunk completion ({} ns >= {} ns), switching to last chunk, written: {}", tt, UPLINK_DURATION_NS, written);
                    return Ok(written);
                } else {
                    state.write_pos = 0;

                    state.read_pos = 0;
                    
                    // After sending each chunk, server sends TIME BYTES
                    // Switch to readable to receive TIME BYTES from server
                    state.phase = TestPhase::PutReceiveTimeBytes;
                    state
                        .stream
                        .reregister(&poll, state.token, Interest::READABLE)?;
                    
                    // Try to read immediately - data might already be available
                    // This handles the case where server sent data while we were writing
                    match handle_put_receive_time_bytes(poll, state) {
                        Ok(n) => return Ok(written + n),
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            // No data available yet, will be handled by next readable event
                            return Ok(written);
                        }
                        Err(e) => return Err(e),
                    }
                }
            }
        }
    } else {
        return Ok(0);
    }
}

pub fn handle_put_receive_time_bytes(
    poll: &Poll,
    state: &mut MeasurementState,
) -> Result<usize, std::io::Error> {
    debug!("handle_put_receive_time_bytes token {:?}", state.token);

    loop {
        debug!("reading time bytes, read_pos: {}", state.read_pos);
        let n = match state.stream.read(&mut state.read_buffer[state.read_pos..]) {
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // No data available yet, return to allow poll() to check again
                debug!("WouldBlock - no data available, returning");
                return Err(e);
            }
            Err(e) => return { debug!("Error reading time bytes: {}", e); Err(e) },
        };
        
        debug!("read {} bytes", n);
        
        state.read_pos += n;

        debug!("read_pos: {}", state.read_pos);

        let buffer_str = String::from_utf8_lossy(&state.read_buffer[..state.read_pos]);

        debug!("buffer_str: {}", buffer_str);

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

        // Continue sending chunks
        state.phase = TestPhase::PutSendChunks;
        state.read_pos = 0;

        state
            .stream
            .reregister(&poll, state.token, Interest::WRITABLE)?;
        return Ok(n);
        }
        
        // No complete line yet, continue reading in the loop
        // The loop will call read() again, which will return WouldBlock if no data available
    }
}

pub fn handle_put_send_last_chunk(
    poll: &Poll,
    state: &mut MeasurementState,
) -> Result<usize, std::io::Error> {
    trace!("handle_put_send_last_chunk token {:?}", state.token);
    let buffer = CHUNK_TERMINATION_STORAGE
        .get(&(state.chunk_size as u64))
        .unwrap();

    loop {
        // Write from current position
        let n = state.stream.write(&buffer[state.write_pos..])?;
        state.bytes_sent += n as u64;
        state.write_pos += n;
        if state.write_pos == state.chunk_size {
            state.phase = TestPhase::PutReceiveFinalTime;
            state.stream.reregister(
                &poll,
                state.token,
                Interest::READABLE,
            )?;
            return Ok(n);
        }
    }
}

pub fn handle_put_receive_final_time(
    poll: &Poll,
    state: &mut MeasurementState,
) -> Result<usize, std::io::Error> {
    trace!("handle_put_receive_final_time token {:?}", state.token);

    loop {
        let n = state
            .stream
            .read(&mut state.read_buffer[state.read_pos..])?;
        state.read_pos += n;

        let buffer_str = String::from_utf8_lossy(&state.read_buffer[..state.read_pos]);

        // Check for ACCEPT message - this indicates the final TIME has been sent
        if buffer_str.contains("ACCEPT GETCHUNKS GETTIME PUT PUTNORESULT PING QUIT\n") {
            // Process all messages in buffer before ACCEPT
            let accept_pos = buffer_str.find("ACCEPT GETCHUNKS GETTIME PUT PUTNORESULT PING QUIT\n").unwrap();
            let before_accept = &buffer_str[..accept_pos];
            
            // Process all TIME BYTES messages before ACCEPT
            let mut processed_pos = 0;
            while let Some(newline_pos) = before_accept[processed_pos..].find('\n') {
                let full_pos = processed_pos + newline_pos;
                let message = &before_accept[processed_pos..full_pos + 1];
                
                // Check if it's a TIME BYTES message
                if message.starts_with("TIME ") && message.contains(" BYTES ") {
                    if let Some(time_bytes) = parse_time_bytes_response(message) {
                        let (time_ns, bytes) = time_bytes;
                        debug!("Parsed TIME BYTES from final buffer: TIME {} BYTES {}", time_ns, bytes);
                        state.upload_measurements.push_back((time_ns, bytes));
                    }
                }
                
                processed_pos = full_pos + 1;
            }
            
            // Find the last TIME line (without BYTES) before ACCEPT
            let mut last_time_pos = None;
            let mut search_pos = 0;
            while let Some(time_pos) = before_accept[search_pos..].find("TIME ") {
                let full_pos = search_pos + time_pos;
                // Check if this is "TIME <t>\n" (not "TIME <t> BYTES")
                if let Some(newline_pos) = before_accept[full_pos..].find('\n') {
                    let time_line = &before_accept[full_pos..full_pos + newline_pos];
                    // If it doesn't contain "BYTES", it's the final TIME
                    if !time_line.contains("BYTES") {
                        last_time_pos = Some(full_pos);
                    }
                }
                search_pos = full_pos + 5; // Move past "TIME "
            }
            
            if let Some(time_pos) = last_time_pos {
                if let Some(newline_pos) = before_accept[time_pos..].find('\n') {
                    let time_line = &before_accept[time_pos..time_pos + newline_pos];
                    if let Some(time_ns) = parse_time_response(time_line) {
                        debug!("Received final TIME {} token {:?}", time_ns, state.token);
                        state.upload_time = Some(time_ns);
                    } else {
                        debug!("Failed to parse final TIME from line: {}", time_line);
                    }
                }
            } else {
                debug!("No final TIME found before ACCEPT message");
            }

            // Print last 20 pairs from upload_measurements
            let total_measurements = state.upload_measurements.len();
            let start_idx = if total_measurements > 20 {
                total_measurements - 20
            } else {
                0
            };
            debug!("Total upload_measurements: {}, showing last {} pairs:", total_measurements, total_measurements - start_idx);
            for (idx, (time, bytes)) in state.upload_measurements.iter().enumerate().skip(start_idx) {
                debug!("  [{}] TIME: {} ns ({:.3} s), BYTES: {} ({:.2} MB)", 
                    idx, 
                    time, 
                    *time as f64 / 1_000_000_000.0,
                    bytes,
                    *bytes as f64 / (1024.0 * 1024.0));
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
        // Process any complete TIME BYTES messages we've received so far
        let mut processed_pos = 0;
        while let Some(newline_pos) = buffer_str[processed_pos..].find('\n') {
            let full_pos = processed_pos + newline_pos;
            let message = &buffer_str[processed_pos..full_pos + 1];
            
            // Check if it's a TIME BYTES message
            if message.starts_with("TIME ") && message.contains(" BYTES ") {
                if let Some(time_bytes) = parse_time_bytes_response(message) {
                    let (time_ns, bytes) = time_bytes;
                    debug!("Parsed TIME BYTES while reading final time: TIME {} BYTES {}", time_ns, bytes);
                    state.upload_measurements.push_back((time_ns, bytes));
                }
            }
            
            processed_pos = full_pos + 1;
        }
        
        // Clear processed data (move remaining data to start of buffer)
        if processed_pos > 0 {
            drop(buffer_str); // Drop borrow before modifying buffer
            let remaining = state.read_pos - processed_pos;
            if remaining > 0 {
                state
                    .read_buffer
                    .copy_within(processed_pos..state.read_pos, 0);
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
        debug!("time_start: {}", time_start);
        if let Some(bytes_start) = buffer_str[time_start..].find(" BYTES ") {
            debug!("bytes_start: {}", bytes_start);
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
                debug!("time: {}, bytes: {}", time, bytes);
                return Some((time, bytes));
            }
        }
    }
    None
}
