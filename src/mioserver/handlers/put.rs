use std::{io, time::Instant};

use log::{debug};
use mio::{Interest, Poll};

use crate::mioserver::{server::TestState, ServerTestPhase};

pub fn handle_put_send_ok(poll: &Poll, state: &mut TestState) -> io::Result<usize> {
    debug!("handle_put_send_ok");
    let command = b"OK\n";
    if state.write_pos == 0 {
        state.write_buffer[0..command.len()].copy_from_slice(command);
        state.write_pos = 0;
    }
    if state.clock.is_none() {
        state.clock = Some(Instant::now());
    }
    loop {
        let n = state.stream.write(&state.write_buffer[state.write_pos..command.len()])?;
        if n == 0 {
            return Err(io::Error::new(io::ErrorKind::Other, "EOF"));
        }
        state.write_pos += n;
        if state.write_pos == command.len() {
            state.write_pos = 0;
            state.measurement_state = ServerTestPhase::PutReceiveChunk;
            state.read_pos = 0;
            //TODO: remove this
            state.chunk_buffer = vec![0u8; state.chunk_size as usize];

            state
                .stream
                .reregister(poll, state.token, Interest::READABLE)?;
            return Ok(n);
        }
    }
}

pub fn handle_put_send_bytes(poll: &Poll, state: &mut TestState) -> io::Result<usize> {
    debug!("handle_put_send_bytes");
    let command = format!("TIME {} BYTES {}\n", state.time_ns.unwrap(), state.total_bytes);
    if state.write_pos == 0 {
        state.write_buffer[0..command.len()].copy_from_slice(command.as_bytes());
        state.write_pos = 0;
    }
    let command_len = command.len();
    loop {
        debug!("Writing bytes {}", state.write_pos);
        let n = state.stream.write(&state.write_buffer[state.write_pos..command_len])?;
        if n == 0 {
            return Err(io::Error::new(io::ErrorKind::Other, "EOF"));
        }
        state.write_pos += n;
        if state.write_pos == command_len {
            if state.chunk_buffer[state.read_pos - 1] == 0xFF {
                state.measurement_state = ServerTestPhase::PutSendTime;
                state.write_pos = 0;
                state
                    .stream
                    .reregister(poll, state.token, Interest::WRITABLE)?;
                return Ok(n);
            } 
            state.read_pos = 0;
            state.write_pos = 0;
            state.measurement_state = ServerTestPhase::PutReceiveChunk;
            state
                .stream
                .reregister(poll, state.token, Interest::READABLE)?;
            return Ok(n);
        }
    }
}

pub fn handle_put_receive_chunk(
    poll: &Poll,
    state: &mut TestState,
) -> io::Result<usize> {
    debug!("handle_put_receive_chunk");
    loop {
        let n = state
            .stream
            .read(&mut state.chunk_buffer[state.read_pos..])?;
        if n == 0 {
            return Err(io::Error::new(io::ErrorKind::Other, "EOF"));
        }
        state.read_pos += n;
        state.total_bytes += n as u64;
        if state.read_pos == state.chunk_size {
            state.measurement_state = ServerTestPhase::PutSendBytes;
            state.time_ns = Some(state.clock.unwrap().elapsed().as_nanos());
            state
                .stream
                .reregister(poll, state.token, Interest::WRITABLE)?;
            return Ok(n);
        }
    }
}

pub fn handle_put_send_time(poll: &Poll, state: &mut TestState) -> io::Result<usize> {
    debug!("handle_put_send_time");
    let command = format!("TIME {}\n", state.time_ns.unwrap());
    if state.write_pos == 0 {
        state.write_buffer[0..command.len()].copy_from_slice(command.as_bytes());
        state.write_pos = 0;
    }
    let command_len = command.len();

    loop {
        debug!("Writing time {}", state.write_pos);
        let n = state.stream.write(&state.write_buffer[state.write_pos..command_len])?;
        if n == 0 {
            return Err(io::Error::new(io::ErrorKind::Other, "EOF"));
        }
        state.write_pos += n;
        if state.write_pos == command_len {
            state.write_pos = 0;
            state.read_pos = 0;
            state.measurement_state = ServerTestPhase::AcceptCommandReceive;
            state
                .stream
                .reregister(poll, state.token, Interest::READABLE)?;
            return Ok(n);
        }
    }
}
