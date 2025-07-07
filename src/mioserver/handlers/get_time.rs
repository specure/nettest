use std::{io, time::Instant};

use log::debug;
use mio::{Interest, Poll};

use crate::{client::globals::{CHUNK_STORAGE, CHUNK_TERMINATION_STORAGE}, mioserver::{server::TestState, ServerTestPhase}};

pub fn handle_get_time_send_chunk(poll: &Poll, state: &mut TestState) -> io::Result<()> {
    debug!("handle_get_time_send_chunk");
    let chunk_size = state.chunk_size;
    let duration = state.duration;

    if state.clock.is_none() {
        state.clock = Some(Instant::now());
    }

    let is_last = state.clock.unwrap().elapsed().as_nanos() > duration as u128 * 1000000000 ;

    let chunk = if is_last {
        CHUNK_TERMINATION_STORAGE.get(&(chunk_size as u64)).unwrap()
    } else {
        CHUNK_STORAGE.get(&(chunk_size as u64)).unwrap()
    };
    loop {
        match state.stream.write(&chunk[state.write_pos..]) {
            Ok(n) => {
                state.write_pos += n;
                debug!("write_pos: {}", state.write_pos);
                if state.write_pos == chunk.len() {
                    state.write_pos = 0;
                    if is_last {
                        state.measurement_state = ServerTestPhase::GetTimeReceiveOk;
                        state.read_pos = 0;
                        if let Err(e) = state
                            .stream
                            .reregister(poll, state.token, Interest::READABLE)
                        {
                            return Err(io::Error::new(io::ErrorKind::Other, e));
                        }
                        return Ok(());
                    }
                    return Ok(());
                }
                return Ok(());
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                return Ok(());
            }
            Err(e) => {
                return Err(e);
            }
        }
    }
}


pub fn handle_get_time_receive_ok(poll: &Poll, state: &mut TestState) -> io::Result<()> {
    debug!("handle_get_time_receive_ok");
    loop {
        match state.stream.read(&mut state.read_buffer) {
            Ok(n) => {
               state.read_pos += n;
               if state.read_buffer[0..state.read_pos] == b"OK\n"[..] {
                state.measurement_state = ServerTestPhase::GetTimeSendTime;
                state.time_ns = Some(state.clock.unwrap().elapsed().as_nanos());
                state.read_pos = 0;
                if let Err(e) = state
                    .stream
                    .reregister(poll, state.token, Interest::WRITABLE)
                {
                    return Err(io::Error::new(io::ErrorKind::Other, e));
                }
                return Ok(());
               }
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                return Ok(());
            }
            Err(e) => {
                return Err(e);
            }
        }
    }
}

pub fn handle_get_time_send_time(poll: &Poll, state: &mut TestState) -> io::Result<()> {
    debug!("handle_get_time_send_time");
    let time_ns = state.time_ns.unwrap();
    state.clock = None;
    let time_response = format!("TIME {}\n", time_ns);

    loop {
        match state.stream.write(&time_response.as_bytes()) {
            Ok(n) => {
                state.write_pos += n;
                if state.write_pos == time_response.len() {
                    state.write_pos = 0;
                    state.measurement_state = ServerTestPhase::AcceptCommandSend;
                    if let Err(e) = state
                        .stream
                        .reregister(poll, state.token, Interest::WRITABLE)
                    {
                        return Err(io::Error::new(io::ErrorKind::Other, e));
                    }
                    return Ok(());
                }
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                return Ok(());
            }
            Err(e) => {
                return Err(e);
            }
        }
    }
}