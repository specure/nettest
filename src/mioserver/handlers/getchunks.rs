use log::{debug, trace};
use mio::{Interest, Poll};
use std::{io, time::Instant};

use crate::{
    client::globals::{CHUNK_STORAGE, CHUNK_TERMINATION_STORAGE},
    mioserver::{server::TestState, ServerTestPhase},
};

pub fn handle_get_chunks_send_ok(poll: &Poll, state: &mut TestState) -> io::Result<usize> {
    debug!("handle_get_chunks_send_ok token {:?}", state.token);
    if state.write_pos == 0 {
        let ok: &'static [u8; 3] = b"OK\n";
        state.write_buffer[..ok.len()].copy_from_slice(ok);
    }
    loop {
        let n = state
            .stream
            .write(&state.write_buffer[state.write_pos..3])?;
        state.write_pos += n;
        if state.write_pos == 3 {
            state.write_pos = 0;
            state.measurement_state = ServerTestPhase::GetChunkSendChunk;
            state
                .stream
                .reregister(poll, state.token, Interest::WRITABLE)?;
            return Ok(n);
        }
    }
}

pub fn handle_get_chunks_send_chunks(poll: &Poll, state: &mut TestState) -> io::Result<usize> {
    debug!("handle_get_chunks_send_chunks token {:?}", state.token);
    let chunk_size = state.chunk_size;
    let chunk_num = state.num_chunks;
    if state.clock.is_none() {
        state.write_pos = 0;
        state.clock = Some(Instant::now());
    }
    let chunk = CHUNK_STORAGE.get(&(chunk_size as u64)).unwrap();
    let is_last = state.processed_chunks == chunk_num - 1;
    if is_last {
        state.measurement_state = ServerTestPhase::GetChunksSendChunksLast;
        state.stream.reregister(poll, state.token, Interest::WRITABLE)?;
        return Ok(1);
    }
    loop {
        debug!("Sending chunk: {} token {:?}", state.processed_chunks, state.token);

        let n = state.stream.write(&chunk[state.write_pos..])?;
        if n == 0 {
            return Ok(0);
        }
        state.write_pos += n;
        if state.write_pos == chunk.len() {
            debug!("Sent chunk: {} token {:?}", state.processed_chunks, state.token);
            state.processed_chunks += 1;
            state.write_pos = 0;
            if state.processed_chunks == chunk_num - 1 {
                debug!("Last chunk transferred token {:?}", state.token);
                state.measurement_state = ServerTestPhase::GetChunksSendChunksLast;
                 state
                    .stream
                    .reregister(poll, state.token, Interest::WRITABLE)?;

                return Ok(n);
            }
        }
    
    }
}

pub fn handle_get_chunks_send_chunks_last(poll: &Poll, state: &mut TestState) -> io::Result<usize> {
    debug!("handle_get_chunks_send_chunks_last token {:?}", state.token);
    let chunk_size = state.chunk_size;
    let chunk = CHUNK_TERMINATION_STORAGE.get(&(chunk_size as u64)).unwrap();
    loop {
        trace!("Sending last chunk: {}", state.processed_chunks);

        let n = state.stream.write(&chunk[state.write_pos..])?;
        if n == 0 {
            return Ok(0);
        }
        state.write_pos += n;
        if state.write_pos == chunk.len() {
            trace!("Sent last chunk: {}", state.processed_chunks);
            state.processed_chunks += 1;
            state.write_pos = 0;
            trace!("Last chunk sent");
            state.measurement_state = ServerTestPhase::GetChunksReceiveOK;
            state.processed_chunks = 0;
            state
                .stream
                .reregister(poll, state.token, Interest::READABLE)?;
            return Ok(n);
        }
    }
}

pub fn handle_get_chunks_receive_ok(poll: &Poll, state: &mut TestState) -> io::Result<usize> {
    trace!("handle_get_chunks_receive_ok");
    loop {
        let n = state.stream.read(&mut state.read_buffer)?;
        state.read_pos += n;
        if state.read_buffer[..state.read_pos] == b"OK\n"[..] {
            state.measurement_state = ServerTestPhase::GetChunksSendTime;
            state.time_ns = Some(state.clock.unwrap().elapsed().as_nanos());
            state.read_pos = 0;
            state
                .stream
                .reregister(poll, state.token, Interest::WRITABLE)?;
            return Ok(n);
        }
    }
}

pub fn handle_get_chunks_send_time(poll: &Poll, state: &mut TestState) -> io::Result<usize> {
    debug!("handle_get_chunks_send_time");

    let time_ns = state.time_ns.unwrap();
    state.clock = None;
    let time_response = format!("TIME {}\n", time_ns);

    loop {
        let n = state.stream.write(&time_response.as_bytes())?;
        if n == 0 {
            return Ok(0);
        }
        state.write_pos += n;
        if state.write_pos == time_response.len() {
            state.write_pos = 0;
            state.measurement_state = ServerTestPhase::AcceptCommandSend;
            state
                .stream
                .reregister(poll, state.token, Interest::WRITABLE)?;
            return Ok(n);
        }
    }
}
