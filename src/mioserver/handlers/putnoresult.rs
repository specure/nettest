use std::{io, time::Instant};

use log::trace;
use mio::{Interest, Poll};

use crate::mioserver::{server::TestState, ServerTestPhase};

pub fn handle_put_no_result_send_ok(poll: &Poll, state: &mut TestState) -> io::Result<usize> {
    trace!("handle_put_no_result_send_ok");
    let command = b"OK\n";
    
    if state.write_pos == 0 {
        state.write_buffer[0..command.len()].copy_from_slice(command);
        state.write_pos = 0;
    }
    loop {
        let n = state.stream.write(&state.write_buffer[0..command.len()])?;
        state.write_pos += n;
        if state.write_pos == command.len() {
            state.write_pos = 0;
            state.measurement_state = ServerTestPhase::PutNoResultReceiveChunk;
            state.read_pos = 0;
            //TODO: remove this
            state.chunk_buffer = vec![0u8; state.chunk_size as usize];
            state.clock = Some(Instant::now());

            state
                .stream
                .reregister(poll, state.token, Interest::READABLE)?;
            return Ok(n);
        }
    }
}

pub fn handle_put_no_result_receive_chunk(
    poll: &Poll,
    state: &mut TestState,
) -> io::Result<(usize)> {
    trace!("handle_put_no_result_receive_chunk");
    loop {
        let n = state
            .stream
            .read(&mut state.chunk_buffer[state.read_pos..])?;
        state.read_pos += n;
        if state.read_pos == state.chunk_size {
            if state.chunk_buffer[state.read_pos - 1] == 0xFF {
                state.time_ns = Some(state.clock.unwrap().elapsed().as_nanos());
                state.measurement_state = ServerTestPhase::PutNoResultSendTime;
                state
                    .stream
                    .reregister(poll, state.token, Interest::WRITABLE)?;
                return Ok(n);
            }
            state.read_pos = 0;
        }
    }
}

pub fn handle_put_no_result_send_time(poll: &Poll, state: &mut TestState) -> io::Result<usize> {
    trace!("handle_put_no_result_send_time");
    let command = format!("TIME {}\n", state.time_ns.unwrap());
    trace!("command: {}", command);
    if state.write_pos == 0 {
        state.write_buffer[0..command.len()].copy_from_slice(command.as_bytes());
        state.write_pos = 0;
    }
    let command_len = command.len();

    loop {
        let n = state.stream.write(&state.write_buffer[0..command_len])?;
        state.write_pos += n;
        if state.write_pos == command_len {
            trace!("command sent");
            state.write_pos = 0;
            state.read_pos = 0;
            state.measurement_state = ServerTestPhase::AcceptCommandSend;
            state
                .stream
                .reregister(poll, state.token, Interest::WRITABLE)?;
            return Ok(n);
        }
    }
}
