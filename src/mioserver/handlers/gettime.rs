use std::{io, time::Instant};

use log::trace;
use mio::{Interest, Poll};

use crate::{
    client::globals::{get_chunk, CHUNK_STORAGE, CHUNK_TERMINATION_STORAGE},
    mioserver::{server::TestState, ServerTestPhase},
};

pub fn handle_get_time_send_chunk(poll: &Poll, state: &mut TestState) -> io::Result<usize> {
    trace!("handle_get_time_send_chunk");
    let chunk_size = state.chunk_size;
    let duration = state.duration;

    if state.clock.is_none() {
        state.clock = Some(Instant::now());
    }

    let is_last = state.clock.unwrap().elapsed().as_nanos() > duration as u128 * 1000000000;

    let chunk = if is_last {
        CHUNK_TERMINATION_STORAGE.get(&(chunk_size as u64)).unwrap_or_else(
            || {
                if state.terminal_chunk.is_none() {
                    let chunk = get_chunk(chunk_size as u64, true);
                    state.terminal_chunk = Some(chunk);
                }
                state.terminal_chunk.as_ref().unwrap()
            }
        )
    } else {
        CHUNK_STORAGE.get(&(chunk_size as u64)).unwrap_or_else(|| {
            if state.chunk.is_none() {
                let chunk = get_chunk(chunk_size as u64, false);
                state.chunk = Some(chunk);
            }
            state.chunk.as_ref().unwrap()

        })
    };
    loop {
        let n = state.stream.write(&chunk[state.write_pos..])?;
        state.write_pos += n;
        if state.write_pos == chunk.len() {
            state.write_pos = 0;
            if is_last {
                trace!("is_last");
                state.measurement_state = ServerTestPhase::GetTimeReceiveOk;
                state.read_pos = 0;
                state.stream.reregister(poll, state.token, Interest::READABLE)?;
                return Ok(n);
            }
            return Ok(n);
        }
    }
}

pub fn handle_get_time_receive_ok(poll: &Poll, state: &mut TestState) -> io::Result<usize> {
    trace!("handle_get_time_receive_ok");
    loop {
        let n = state.stream.read(&mut state.read_buffer)?;
        state.read_pos += n;
        if state.read_buffer[0..state.read_pos] == b"OK\n"[..] {
            state.measurement_state = ServerTestPhase::GetTimeSendTime;
            state.time_ns = Some(state.clock.unwrap().elapsed().as_nanos());
            state.read_pos = 0;
            state.stream.reregister(poll, state.token, Interest::WRITABLE)?;
            return Ok(n);
        }
    }
}

pub fn handle_get_time_send_time(poll: &Poll, state: &mut TestState) -> io::Result<usize> {
    trace!("handle_get_time_send_time");
    let time_ns = state.time_ns.unwrap();
    state.clock = None;
    let time_response = format!("TIME {}\n", time_ns);

    loop {
        let n = state.stream.write(&time_response.as_bytes())?;
        state.write_pos += n;
        if state.write_pos == time_response.len() {
            state.write_pos = 0;
            state.measurement_state = ServerTestPhase::AcceptCommandSend;
            state.stream.reregister(poll, state.token, Interest::WRITABLE)?;
            return Ok(n);
        }
    }
}
