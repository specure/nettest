use std::io;

use log::{trace};
use mio::{Interest, Poll};

use crate::mioserver::{server::TestState, ServerTestPhase};

pub fn handle_put_no_result_send_ok(poll: &Poll, state: &mut TestState) -> io::Result<()> {
    trace!("handle_put_no_result_send_ok");
    let command = b"OK\n";
    if state.write_pos == 0 {
        state.write_buffer[0..command.len()].copy_from_slice(command);
        state.write_pos = 0;
    }
    let command_len = command.len();
    loop {
        match state.stream.write(&state.write_buffer[0..command_len]) {
            Ok(n) if n > 0 => {
                state.write_pos += n;
                if state.write_pos == command_len {
                    state.write_pos = 0;
                    state.measurement_state = ServerTestPhase::PutNoResultReceiveChunk;
                    state.read_pos = 0;

                    //TODO: remove this
                    state.chunk_buffer = vec![0u8; state.chunk_size as usize];

                    if let Err(e) = state.stream.reregister(poll, state.token, Interest::READABLE) {
                        return Err(io::Error::new(io::ErrorKind::Other, e));
                    }
                    return Ok(());
                }
            }
            Ok(_) => {
                return Err(io::Error::new(io::ErrorKind::Other, "Connection closed"));
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

pub fn handle_put_no_result_receive_chunk(poll: &Poll, state: &mut TestState) -> io::Result<()> {
    trace!("handle_put_no_result_receive_chunk");
    loop {
        match state.stream.read(&mut state.chunk_buffer[state.read_pos..]) {
            Ok(n) if n > 0 => {
                state.read_pos += n;
                if state.read_pos == state.chunk_size  {
                    if state.chunk_buffer[state.read_pos - 1] == 0xFF {
                        state.measurement_state = ServerTestPhase::PutNoResultSendTime;
                        if let Err(e) = state.stream.reregister(poll, state.token, Interest::WRITABLE) {
                            return Err(io::Error::new(io::ErrorKind::Other, e));
                        }
                        return Ok(());
                    } else if state.chunk_buffer[state.read_pos - 1] == 0x00 {
                        state.read_pos = 0;
                        return  handle_put_no_result_receive_chunk(poll, state);
                    }
                }
            }
            Ok(_) => {
                return Err(io::Error::new(io::ErrorKind::Other, "Connection closed"));
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

pub fn handle_put_no_result_send_time(poll: &Poll, state: &mut TestState) -> io::Result<()> { 
    trace!("handle_put_no_result_send_time");
    let command = format!("TIME {}\n", state.time_ns.unwrap());
    if state.write_pos == 0 {
        state.write_buffer[0..command.len()].copy_from_slice(command.as_bytes());
        state.write_pos = 0;
    }
    let command_len = command.len();
    
    loop {
        match state.stream.write(&state.write_buffer[0..command_len]) {
            Ok(n) => {
                state.write_pos += n;
                if state.write_pos == command_len {
                    state.write_pos = 0;
                    state.read_pos = 0;
                    state.measurement_state = ServerTestPhase::AcceptCommandReceive;
                    if let Err(e) = state.stream.reregister(poll, state.token, Interest::READABLE) {
                        return Err(io::Error::new(io::ErrorKind::Other, e));
                    }
                    return Ok(());
                }
            }
            Ok(_) => {
                return Err(io::Error::new(io::ErrorKind::Other, "Connection closed"));
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