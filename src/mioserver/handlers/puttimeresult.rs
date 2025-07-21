use std::{io, time::Instant};

use log::{debug, info, trace};
use mio::{Interest, Poll};

use crate::{
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
            let tt = state.clock.unwrap().elapsed().as_nanos();
            state
                    .bytes_received
                    .push_back((tt as u64, state.total_bytes));
            if state.chunk_buffer[state.read_pos - 1] == 0xFF {
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
    info!("handle_put_time_result_send_time");
    let result = state.bytes_received.iter().map(|(t, b)| format!("({} {})", t, b)).collect::<Vec<String>>().join("; ");
    let command = format!("TIMERESULT {}\n", result);
    if state.write_pos == 0 {
        if state.chunk_buffer.len() < command.len() {
            state.chunk_buffer.resize(command.len(), 0);
        }
        state.chunk_buffer[0..command.len()].copy_from_slice(command.as_bytes());
    }
    loop {
        let n = state
            .stream
            .write(&state.chunk_buffer[state.write_pos..])?;
        if n == 0 {
            return Err(io::Error::new(io::ErrorKind::Other, "EOF"));
        }
        state.write_pos += n;
        info!("write_pos: {}", state.write_pos);
        if state.write_pos == state.chunk_buffer.len() {
            let tt = state.clock.unwrap().elapsed().as_nanos();
            state.total_bytes += state.chunk_buffer.len() as u64;
            state
                    .bytes_received
                    .push_back((tt as u64, state.total_bytes));
            debug!("command sent");
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
