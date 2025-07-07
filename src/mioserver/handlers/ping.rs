use log::trace;
use mio::{Interest, Poll};
use std::{io, time::Instant};

use crate::mioserver::{server::TestState, ServerTestPhase};

pub fn handle_pong_send(poll: &Poll, state: &mut TestState) -> io::Result<()> {
    trace!("handle_pong_send");
    let pong = b"PONG\n";
    if state.write_pos == 0 {
        state.write_buffer[..pong.len()].copy_from_slice(pong);
    }
    if state.clock.is_none() {
        state.clock = Some(Instant::now());
    }
    loop {
        match state
            .stream
            .write(&state.write_buffer[state.write_pos..pong.len()])
        {
            Ok(n) => {
                state.write_pos += n;
                if state.write_pos == pong.len() {
                    if let Err(e) = state
                        .stream
                        .reregister(poll, state.token, Interest::READABLE)
                    {
                        return Err(io::Error::new(io::ErrorKind::Other, e));
                    }
                    state.write_pos = 0;
                    state.measurement_state = ServerTestPhase::PingReceiveOk;
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

pub fn handle_ping_receive_ok(poll: &Poll, state: &mut TestState) -> io::Result<()> {
    trace!("handle_ping_receive_ok");
    let ok = b"OK\n";
    loop {
        match state.stream.read(&mut state.read_buffer) {
            Ok(n) => {
                if n == 0 {
                    return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "EOF"));
                }
                state.read_pos += n;
                if state.read_buffer[0..ok.len()] == ok[..] {
                    let time = state.clock.unwrap().elapsed().as_nanos();
                    state.clock = None;
                    state.time_ns = Some(time);
                    state.measurement_state = ServerTestPhase::PingSendTime;
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

pub fn handle_ping_send_time(poll: &Poll, state: &mut TestState) -> io::Result<()> {
    trace!("handle_ping_send_time");
    let time = format!("TIME {}\n", state.time_ns.unwrap());
    if state.write_pos == 0 {
        state.write_buffer[..time.len()].copy_from_slice(time.as_bytes());
    }
    loop {
        match state
            .stream
            .write(&state.write_buffer[state.write_pos..time.len()])
        {
            Ok(n) => {
                state.write_pos += n;
                if state.write_pos == time.len() {
                    state.write_pos = 0;
                    state.measurement_state = ServerTestPhase::AcceptCommandSend;
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
