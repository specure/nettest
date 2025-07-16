use std::{io, time::Instant};

use log::{debug, trace};
use mio::{Interest, Poll};

use crate::{
    client::state::ONE_SECOND_NS,
    mioserver::{server::TestState, ServerTestPhase},
};

pub fn handle_put_time_result_send_ok(poll: &Poll, state: &mut TestState) -> io::Result<usize> {
    debug!("handle_put_time_result_send_ok");
    let command = b"OK\n";
    if state.clock.is_none() {
        state.write_pos = 0;
        state.clock = Some(Instant::now());
    }
    if state.write_pos == 0 {
        state.write_buffer[0..command.len()].copy_from_slice(command);
        state.write_pos = 0;
    }
    loop {
        let n = state
            .stream
            .write(&state.write_buffer[state.write_pos..command.len()])?;
        if n == 0 {
            return Err(io::Error::new(io::ErrorKind::Other, "EOF"));
        }
        state.write_pos += n;

        if state.write_pos == command.len() {
            state.write_pos = 0;
            state.measurement_state = ServerTestPhase::PutTimeResultReceiveChunk;
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

pub fn handle_put_time_result_receive_chunk(
    poll: &Poll,
    state: &mut TestState,
) -> io::Result<(usize)> {
    debug!("handle_put_time_result_receive_chunk");
    loop {
        let n = state
            .stream
            .read(&mut state.chunk_buffer[state.read_pos..])?;
        if n == 0 {
            debug!("EOF");
            return Err(io::Error::new(io::ErrorKind::Other, "EOF"));
        }
        state.read_pos += n;
        state.total_bytes += n as u64;
        trace!("Read {} bytes", state.read_pos);
        if state.read_pos == state.chunk_size {
            trace!("Chunk size reached");
            if state.chunk_buffer[state.read_pos - 1] == 0xFF {
                trace!("Last byte is 0xFF");
                let tt = state.clock.unwrap().elapsed().as_nanos();
                state
                    .bytes_received
                    .push_back((tt as u64, state.total_bytes));
                state.time_ns = Some(state.clock.unwrap().elapsed().as_nanos());
                state.measurement_state = ServerTestPhase::PutTimeResultSendTimeResult;
                state.read_pos = 0;
                state.write_pos = 0;
                state
                    .stream
                    .reregister(poll, state.token, Interest::WRITABLE)?;
                return Ok(n);
            } else {
                if state.chunk_buffer[state.read_pos - 1] != 0x00 {
                    return Err(io::Error::new(io::ErrorKind::Other, "Invalid chunk"));
                }
            }
            state.read_pos = 0;
        }
    }
}

pub fn handle_put_time_result_send_time(poll: &Poll, state: &mut TestState) -> io::Result<usize> {
    debug!("handle_put_time_result_send_time");
    let result = state.bytes_received.iter().map(|(t, b)| format!("({} {})", t, b)).collect::<Vec<String>>().join("; ");
    let command = format!("TIMERESULT {}\n", result);
    if state.write_pos == 0 {
        state.write_buffer[0..command.len()].copy_from_slice(command.as_bytes());
        state.write_pos = 0;
    }
    let command_len = command.len();

    loop {
        let n = state
            .stream
            .write(&state.write_buffer[state.write_pos..command_len])?;
        if n == 0 {
            return Err(io::Error::new(io::ErrorKind::Other, "EOF"));
        }
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
