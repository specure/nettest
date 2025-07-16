use std::io;

use anyhow::Result;
use log::{debug, trace};
use mio::{Interest, Poll};

use crate::{client::constants::{MAX_CHUNK_SIZE, MIN_CHUNK_SIZE}, config::constants::CHUNK_SIZE, mioserver::{server::TestState, ServerTestPhase}};

pub fn handle_greeting_accep_token_read(
    poll: &Poll,
    state: &mut TestState,
) -> Result<usize, std::io::Error> {
    debug!("handle_greeting_accep_token_read");
    loop {
        let n = state
            .stream
            .read(&mut state.read_buffer[state.read_pos..])?;
        state.read_pos += n;
        if state.read_pos >= 4
            && state.read_buffer[state.read_pos - 4..state.read_pos] == [b'\r', b'\n', b'\r', b'\n']
        {
            state.read_pos = 0;
            state.measurement_state = ServerTestPhase::GreetingSendAcceptToken;
            if let Err(e) = state
                .stream
                .reregister(poll, state.token, Interest::WRITABLE)
            {
                return Err(io::Error::new(io::ErrorKind::Other, e));
            }
            return Ok(n);
        }
    }
}


pub fn handle_greeting_send_version(
    poll: &Poll,
    state: &mut TestState,
) -> Result<usize, std::io::Error> {
    debug!("handle_greeting_send_version");
    let version = "RMBTv1.5.0\n";

    if state.write_pos == 0 {
        state.write_buffer[..version.len()].copy_from_slice(version.as_bytes());
    }

    loop {
        let n = state.stream.write(&state.write_buffer[state.write_pos..(version.as_bytes().len())])?;
        if n == 0 {
            return Err(io::Error::new(io::ErrorKind::Other, "EOF"));
        }
        state.write_pos += n;
        debug!("Wrote handle_greeting_send_version {}", n);
        if state.write_pos == (version.as_bytes().len()) {
            state.write_pos = 0;
            state.read_pos = 0;
            state.stream.flush()?;
            state.measurement_state = ServerTestPhase::GreetingSendAcceptToken;
            state.stream.reregister(poll, state.token, Interest::WRITABLE)?;
            return Ok(n);
        }
    }
}


pub fn handle_greeting_send_accept_token(
    poll: &Poll,
    state: &mut TestState,
) -> Result<usize, std::io::Error> {
    debug!("handle_greeting_send_accept_token");
    let accept = "ACCEPT TOKEN QUIT\n".as_bytes();

    if state.write_pos == 0 {
        state.write_buffer[..accept.len()].copy_from_slice(accept);
    }

    loop {
        let n = state.stream.write(&state.write_buffer[state.write_pos..(accept.len())])?;
        if n == 0 {
            return Err(io::Error::new(io::ErrorKind::Other, "EOF"));
        }
        state.write_pos += n;
        debug!("Wrote handle_greeting_send_accept_token {}", n);
        if state.write_pos == (accept.len()) {
            state.write_pos = 0;
            state.read_pos = 0;
            state.stream.flush()?;

            state.measurement_state = ServerTestPhase::GreetingReceiveToken;

            state.stream.reregister(poll, state.token, Interest::READABLE )?;
            return Ok(n);
        }
    }
}



pub fn handle_greeting_receive_token(
    poll: &Poll,
    state: &mut TestState,
) -> Result<usize, std::io::Error> {
    debug!("handle_greeting_receive_token");
    loop {
        let n = state
            .stream
            .read(&mut state.read_buffer[state.read_pos..])?;
        state.read_pos += n;
        let end = b"\n";
        //compare last 2 bytes with end
        //TODO handle properly client uuid
        if n > 0 && state.read_buffer[state.read_pos - 1..state.read_pos] == *end {
            state.measurement_state = ServerTestPhase::GreetingSendOk;
            trace!("Greeting received token: {}", String::from_utf8_lossy(&state.read_buffer));
            state.read_pos = 0;
            state.stream.reregister(poll, state.token, Interest::WRITABLE)?;
            return Ok(n);
        }
    }
}

pub fn handle_greeting_send_ok(
    poll: &Poll,
    state: &mut TestState,
) -> Result<usize, std::io::Error> {
    debug!("handle_greeting_send_ok");
    let ok = b"OK\n";

    if state.write_pos == 0 {
        state.write_buffer[..ok.len()].copy_from_slice(ok);
    }
    loop {
        let n = state.stream.write(&state.write_buffer[state.write_pos..ok.len()])?;
        state.write_pos += n;
        if state.write_pos == ok.len() {
            state.write_pos = 0;
            state.measurement_state = ServerTestPhase::GreetingSendChunksize;
            state.stream.reregister(poll, state.token, Interest::WRITABLE)?;
            return Ok(n);
        }
    }
}

pub fn handle_greeting_send_chunksize( poll: &Poll,
    state: &mut TestState,) -> Result<usize, std::io::Error> {
    debug!("handle_greeting_send_ok");
    let chunk_size_msg = format!("CHUNKSIZE {} {} {}\n", CHUNK_SIZE, MIN_CHUNK_SIZE, MAX_CHUNK_SIZE); //todo compare version

    if state.write_pos == 0 {
        state.write_buffer[..chunk_size_msg.len()].copy_from_slice(chunk_size_msg.as_bytes());
    }
    loop {
        let n = state.stream.write(&state.write_buffer[state.write_pos..chunk_size_msg.len()])?;
        state.write_pos += n;
        if state.write_pos == chunk_size_msg.len() {
            state.write_pos = 0;
            state.measurement_state = ServerTestPhase::AcceptCommandSend;
            state.stream.reregister(poll, state.token, Interest::WRITABLE)?;
            return Ok(n);
        }
    }
}
