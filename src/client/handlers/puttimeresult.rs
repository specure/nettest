use anyhow::Result;
use log::{debug, info, trace};
use crate::reactor::{Interest, Poll};
use std::time::Duration;
use web_time::Instant;

use crate::client::constants::ACCEPT_GETCHUNKS_STRING;
use crate::client::globals::{CHUNK_STORAGE, CHUNK_TERMINATION_STORAGE};
use crate::client::state::{MeasurementState, TestPhase};


/// Parse every complete `\n`-terminated line currently in `time_result_buffer`.
/// Pushes pairs from `TIMERESULT ...` lines into `upload_measurements`.
/// Returns true if the terminal `ACCEPT ...` line was seen.
fn process_timeresult_lines(measurement_state: &mut MeasurementState) -> bool {
    loop {
        let pos = match measurement_state
            .time_result_buffer
            .iter()
            .position(|&b| b == b'\n')
        {
            Some(p) => p,
            None => return false,
        };
        let line =
            String::from_utf8_lossy(&measurement_state.time_result_buffer[..pos]).to_string();

        if line.starts_with("TIMERESULT ") {
            measurement_state.time_result_buffer.drain(..pos + 1);
            let data_part = &line[11..];
            let pairs: Vec<(u64, u64)> = data_part
                .split("; ")
                .filter_map(|pair| {
                    let pair = pair.trim_start_matches('(').trim_end_matches(')');
                    let parts: Vec<&str> = pair.split_whitespace().collect();
                    if parts.len() == 2 {
                        let time = parts[0].parse::<u64>().ok()?;
                        let bytes = parts[1].parse::<u64>().ok()?;
                        Some((time, bytes))
                    } else {
                        None
                    }
                })
                .collect();
            for (time, bytes) in &pairs {
                measurement_state.upload_measurements.push_back((*time, *bytes));
            }
            if let Some((last_time, last_bytes)) = pairs.last() {
                measurement_state.upload_time = Some(*last_time);
                measurement_state.upload_bytes = Some(*last_bytes);
            }
        } else if line.starts_with("ACCEPT ") {
            measurement_state.time_result_buffer.drain(..pos + 1);
            return true;
        } else {
            // Unrecognized line (e.g. binary data from an older server whose
            // reply ends with the ACCEPT terminal): do NOT consume it, so the
            // caller's `ends_with(ACCEPT terminal)` check can detect completion.
            return false;
        }
    }
}

/// During the upload (PUTTIMERESULT) phase the client is busy *sending* chunks,
/// so interim TIMERESULT messages from the server pile up in the socket. This
/// opportunistically drains them (non-blocking) and republishes the live upload
/// samples, throttled to ~100 ms, so the upload graph can grow during the phase.
fn drain_interim_upload(measurement_state: &mut MeasurementState) {
    let now = Instant::now();
    let due = match measurement_state.last_live_publish {
        Some(t) => now.duration_since(t) >= Duration::from_millis(100),
        None => true,
    };
    if !due {
        return;
    }

    // Non-blocking drain of all interim data currently available.
    let mut buf = [0u8; 65536];
    loop {
        match measurement_state.stream.read(&mut buf) {
            Ok(n) if n > 0 => {
                measurement_state.time_result_buffer.extend_from_slice(&buf[..n]);
            }
            _ => break, // WouldBlock / EOF / error: stop draining, keep sending
        }
    }
    let _ = process_timeresult_lines(measurement_state);

    // Publish current upload samples to the live sink.
    if let Some(sink) = &measurement_state.live_sink {
        if let Ok(mut g) = sink.upload.lock() {
            g.clear();
            g.extend(measurement_state.upload_measurements.iter().cloned());
        }
    }
    measurement_state.last_live_publish = Some(now);
}

/// Readable handler active during the upload SEND phases: when interim data
/// arrives while writes are blocked (real networks), a READABLE event lets us
/// drain it promptly instead of waiting for the next chunk boundary.
pub fn handle_put_time_result_drain(
    poll: &Poll,
    measurement_state: &mut MeasurementState,
) -> Result<usize, std::io::Error> {
    let mut buf = [0u8; 65536];
    let mut total = 0usize;
    loop {
        match measurement_state.stream.read(&mut buf) {
            Ok(n) if n > 0 => {
                measurement_state.time_result_buffer.extend_from_slice(&buf[..n]);
                total += n;
            }
            Ok(_) => break, // EOF
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
            Err(e) => return Err(e),
        }
    }
    let accept_seen = process_timeresult_lines(measurement_state);
    if let Some(sink) = &measurement_state.live_sink {
        if let Ok(mut g) = sink.upload.lock() {
            g.clear();
            g.extend(measurement_state.upload_measurements.iter().cloned());
        }
    }
    // Defense-in-depth: if the final ACCEPT arrived while draining, complete the
    // phase here instead of dropping it (which would hang PerfReceiveTime).
    if accept_seen {
        measurement_state.phase = TestPhase::PerfCompleted;
        measurement_state.stream.reregister(
            &poll,
            measurement_state.token,
            Interest::READABLE,
        )?;
        measurement_state.read_pos = 0;
        measurement_state.write_pos = 0;
        measurement_state.time_result_buffer.clear();
    }
    // Never return 0: the phase loop treats 0 as a failed read.
    Ok(total.max(1))
}

pub fn handle_put_time_result_receive_ok(
    poll: &Poll,
    measurement_state: &mut MeasurementState,
) -> Result<usize, std::io::Error> {
    debug!("handle_perf_receive_ok token {:?}", measurement_state.token);
    loop {
        let n = measurement_state
            .stream
            .read(&mut measurement_state.read_buffer[measurement_state.read_pos..b"OK\n".len()])?;
        if n == 0 {
            return Ok(0);
        }
        if n == b"OK\n".len() {
            measurement_state.phase = TestPhase::PerfSendChunks;
            // READABLE too: drain interim TIMERESULT while sending.
            measurement_state.stream.reregister(
                &poll,
                measurement_state.token,
                Interest::READABLE | Interest::WRITABLE,
            )?;
            measurement_state.read_pos = 0;
            return Ok(n);
        }
    }
}

pub fn handle_put_time_result_receive_time(
    poll: &Poll,
    measurement_state: &mut MeasurementState,
) -> Result<usize, std::io::Error> {
    debug!("handle_put_time_result_receive_time token {:?}", measurement_state.token);
    loop {
        let n = measurement_state
            .stream
            .read(&mut measurement_state.read_buffer[measurement_state.read_pos..])?;
        if n == 0 {
            return Ok(0);
        }
        measurement_state.time_result_buffer.extend_from_slice(&measurement_state.read_buffer[..n]);

        // The server may send any number of interim `TIMERESULT ...` lines for
        // the live graph. Completion is detected by the buffer ending with the
        // ACCEPT terminal — this also covers older servers whose PUTTIMERESULT
        // reply is binary data followed by that terminal (no clean lines).
        let completed = process_timeresult_lines(measurement_state)
            || String::from_utf8_lossy(&measurement_state.time_result_buffer)
                .ends_with(ACCEPT_GETCHUNKS_STRING);
        if completed {
            measurement_state.phase = TestPhase::PerfCompleted;
            measurement_state.stream.reregister(
                &poll,
                measurement_state.token,
                Interest::READABLE,
            )?;
            measurement_state.read_pos = 0;
            measurement_state.write_pos = 0;
            measurement_state.time_result_buffer.clear();
            return Ok(n);
        }
    }
}

pub fn handle_put_time_result_send_command(
    poll: &Poll,
    measurement_state: &mut MeasurementState,
) -> Result<usize, std::io::Error> {
    let command = if measurement_state.puttimeresult_interval_ms > 0 {
        format!(
            "PUTTIMERESULT {} {}\n",
            measurement_state.chunk_size, measurement_state.puttimeresult_interval_ms
        )
    } else {
        format!("PUTTIMERESULT {}\n", measurement_state.chunk_size)
    };
    if measurement_state.write_pos == 0 {
        measurement_state.write_buffer[..command.len()].copy_from_slice(command.as_bytes());
    }
    loop {
        let n = measurement_state
            .stream
            .write(&mut measurement_state.write_buffer[measurement_state.write_pos..command.len()])?;
        if n == 0 {
            return Ok(0);
        }
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

pub fn handle_put_time_result_send_chunks(
    poll: &Poll,
    measurement_state: &mut MeasurementState,
) -> Result<usize, std::io::Error> {
    trace!("handle_put_time_result_send_chunks token {:?}", measurement_state.token);
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
                let duration_ns = measurement_state.upload_duration_ms as u128 * 1_000_000;
                let is_last = tt >= duration_ns;

                if is_last {
                    measurement_state.phase = TestPhase::PerfSendLastChunk;
                    // WRITABLE only: do NOT drain on READABLE during the last
                    // chunk. The final TIMERESULT + ACCEPT must be consumed by
                    // PerfReceiveTime; if the interim drainer (which ignores
                    // ACCEPT) ate them here, the phase would hang.
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

            // Drain interim TIMERESULT after every successful write (not just
            // on a full chunk_size boundary): with a large chunk_size and a
            // fast link, this inner loop can write the *entire* upload in one
            // handler invocation without ever returning to the poll loop, so
            // the chunk-boundary-only drain (and the READABLE-driven
            // handle_put_time_result_drain) may never run before the last
            // chunk. Throttled internally to ~100ms so this is cheap.
            drain_interim_upload(measurement_state);
        }
    } else {
        return Ok(0);
    }
}

pub fn handle_put_time_result_send_last_chunk(
    poll: &Poll,
    measurement_state: &mut MeasurementState,
) -> Result<usize, std::io::Error> {
    debug!("handle_perf_send_last_chunk token {:?}", measurement_state.token);
    let buffer = CHUNK_TERMINATION_STORAGE
        .get(&(measurement_state.chunk_size as u64))
        .unwrap();

    loop {
        // Write from current position
        let n = measurement_state.stream.write(&buffer[measurement_state.write_pos..])?;
        if n == 0 {
            return Ok(0);
        }
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
