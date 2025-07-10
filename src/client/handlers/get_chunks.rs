use crate::client::globals::MIN_CHUNK_SIZE;
use crate::client::handlers::BasicHandler;
use crate::client::state::TestPhase;
use crate::client::utils::{
    ACCEPT_GETCHUNKS_STRING, MAX_CHUNKS_BEFORE_SIZE_INCREASE, MAX_CHUNK_SIZE, OK_COMMAND,
    PRE_DOWNLOAD_DURATION_NS, TIME_BUFFER_SIZE,
};
use crate::client::{write_all_nb, MeasurementState, DEFAULT_READ_BUFFER_SIZE};
use anyhow::Result;
use bytes::BytesMut;
use log::{debug, trace};
use mio::{Interest, Poll, Token};
use std::io::{self};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Instant;

pub struct GetChunksHandler {}

impl GetChunksHandler {
    pub fn new(token: Token) -> Result<Self> {
        Ok(Self {})
    }
}

impl BasicHandler for GetChunksHandler {
    fn on_read(&mut self, poll: &Poll, measurement_state: &mut MeasurementState) -> Result<()> {
        match measurement_state.phase {
            TestPhase::GetChunksReceiveChunk => {
                match handle_get_chunks_receive_chunk(poll, measurement_state) {
                    Ok(n) => {
                        return Ok(());
                    }

                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        return Ok(());
                    }
                    Err(e) => {
                        return Err(anyhow::anyhow!("Error receiving chunk: {}", e));
                    }
                }
            }

            TestPhase::GetChunksReceiveTime => {
                match handle_get_chunks_receive_time(poll, measurement_state) {
                    Ok(n) => {
                        return Ok(());
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        return Ok(());
                    }
                    Err(e) => {
                        return Err(anyhow::anyhow!("Error receiving time: {}", e));
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn on_write(&mut self, poll: &Poll, measurement_state: &mut MeasurementState) -> Result<()> {
        match measurement_state.phase {
            TestPhase::GetChunksSendChunksCommand => {
                trace!("GetChunksSendChunksCommand");
                match handle_get_chunks_send_chunks_command(poll, measurement_state) {
                    Ok(n) => {
                        return Ok(());
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        return Ok(());
                    }
                    Err(e) => {
                        return Err(anyhow::anyhow!("Error sending chunks command: {}", e));
                    }
                }
            }
            TestPhase::GetChunksSendOk => {
                trace!("GetChunksSendOk");
                match handle_get_chunks_send_ok(poll, measurement_state) {
                    Ok(n) => {
                        return Ok(());
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        return Ok(());
                    }
                    Err(e) => {
                        return Err(anyhow::anyhow!("Error sending ok command: {}", e));
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }
}

pub fn handle_get_chunks_receive_time(
    poll: &Poll,
    state: &mut MeasurementState,
) -> Result<usize, std::io::Error> {
    debug!("GetChunksReceiveTime");
    loop {
        let n = state
            .stream
            .read(&mut state.read_buffer[state.read_pos..])?;
        state.read_pos += n;
        let buffer_str = String::from_utf8_lossy(&state.read_buffer[..state.read_pos]);

        if buffer_str.contains(ACCEPT_GETCHUNKS_STRING) {
            debug!(
                "GetChunksReceiveTime: {}",
                String::from_utf8_lossy(&state.read_buffer[..state.read_pos])
            );
            if let Some(time_ns) = parse_time_response(&buffer_str) {
                debug!("Time: {} ns", time_ns);
                if time_ns < PRE_DOWNLOAD_DURATION_NS && state.chunk_size < MAX_CHUNK_SIZE as usize
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
    debug!("GetChunksSendChunksCommand");
    let command = format!("GETCHUNKS {} {}\n", state.total_chunks, state.chunk_size);
    debug!("Command: {}", command);
    if state.write_pos == 0 {
        state.write_buffer[0..command.len()].copy_from_slice(command.as_bytes());
    }
    loop {
        debug!("GetChunksSendChunksCommand Loop");

        let n = state
            .stream
            .write(&state.write_buffer[state.write_pos..state.write_pos + command.len()])?;
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
        // state
        //     .stream
        //     .reregister(&poll, state.token, Interest::WRITABLE)?;
    }
}

pub fn handle_get_chunks_send_ok(
    poll: &Poll,
    state: &mut MeasurementState,
) -> Result<usize, std::io::Error> {
    debug!("GetChunksSendOk");
    if state.write_pos == 0 {
        state.write_buffer[0..OK_COMMAND.len()].copy_from_slice(OK_COMMAND);
    }
    loop {
        let n = state
            .stream
            .write(&state.write_buffer[state.write_pos..state.write_pos + OK_COMMAND.len()])?;
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
    debug!("GetChunksReceiveChunk");

    loop {
        let n = state
            .stream
            .read(&mut state.chunk_buffer[state.read_pos..])?;
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
    debug!("Parse time response: {}", buffer_str);
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
        debug!(
            "Increased total chunks to {}",
            measurement_state.total_chunks
        );
    } else {
        measurement_state.chunk_size =
            (measurement_state.chunk_size * 2).min(MAX_CHUNK_SIZE as usize);
        measurement_state
            .chunk_buffer
            .resize(measurement_state.chunk_size as usize, 0);
        debug!("Increased chunk size to {}", measurement_state.chunk_size);
    }
}
