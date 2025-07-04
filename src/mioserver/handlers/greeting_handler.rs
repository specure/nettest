use std::io;

use anyhow::Result;
use log::{debug, trace};
use mio::{Interest, Poll};

use crate::mioserver::{server::TestState, ServerTestPhase};

pub fn handle_greeting_accep_token_read(
    poll: &Poll,
    state: &mut TestState,
) -> Result<(), std::io::Error> {
    debug!("handle_greeting_accep_token_read");
    loop {
        match state.stream.read(&mut state.read_buffer[state.read_pos..]) {
            Ok(n) => {
                state.read_pos += n;
                if state.read_pos >= 4
                    && state.read_buffer[state.read_pos - 4..state.read_pos]
                        == [b'\r', b'\n', b'\r', b'\n']
                {
                    state.read_pos = 0;
                    state.measurement_state = ServerTestPhase::GreetingSendAcceptToken;
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

pub fn handle_greeting_send_accept_token(
    poll: &Poll,
    state: &mut TestState,
) -> Result<(), std::io::Error> {
    debug!("handle_greeting_send_accept_token");
    let version = b"RMBTv1.5.0\n";
    let accept = b"ACCEPT TOKEN QUIT\n";

    if state.write_pos == 0 {
        // Копируем version в начало буфера
        state.write_buffer[..version.len()].copy_from_slice(version);
        // Копируем accept после version
        state.write_buffer[version.len()..version.len() + accept.len()].copy_from_slice(accept);
    }

    match state.stream.write(&state.write_buffer[state.write_pos..]) {
        Ok(n) => {
            state.write_pos += n;
            if state.write_pos == state.write_buffer.len() {
                state.write_pos = 0;
                state.measurement_state = ServerTestPhase::GreetingReceiveToken;
                if let Err(e) = state
                    .stream
                    .reregister(poll, state.token, Interest::READABLE)
                {
                    return Err(io::Error::new(io::ErrorKind::Other, e));
                }
                return Ok(());
            }
        }
        Err(e) => {
            return Err(e);
        }
    }
    Ok(())
}

pub fn handle_greeting_receive_token(
    poll: &Poll,
    state: &mut TestState,
) -> Result<(), std::io::Error> {
    debug!("handle_greeting_receive_token");
    loop {
        match state.stream.read(&mut state.read_buffer[state.read_pos..]) {
            Ok(n) => {
                state.read_pos += n;
                let end = b"\n";
                let line = String::from_utf8_lossy(&state.read_buffer[..state.read_pos]);

                debug!("handle_greeting_receive_token: line received {}", line);

                //compare last 2 bytes with end
                if state.read_buffer[state.read_pos - 1..state.read_pos] == *end {
                    state.read_pos = 0;
                    state.measurement_state = ServerTestPhase::GreetingSendOk;
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

pub fn handle_greeting_send_ok(poll: &Poll, state: &mut TestState) -> Result<(), std::io::Error> {
    debug!("handle_greeting_send_ok");
    let ok = b"OK\r\n";
    if state.write_pos == 0 {
        state.write_buffer[..ok.len()].copy_from_slice(ok);
    }
    loop {
        match state.stream.write(&state.write_buffer[state.write_pos..]) {
            Ok(n) => {
                state.write_pos += n;
                if state.write_pos == state.write_buffer.len() {
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
