use crate::client::{state::TestPhase, MeasurementState};
use crate::stream::stream::Stream;
use anyhow::Result;
use log::{debug};
use mio::{Interest, Poll};


pub fn handle_greeting_send_connection_type(
    poll: &Poll,
    state: &mut MeasurementState,
) -> Result<usize, std::io::Error> {
    debug!("handle_greeting_send_connection_type token {:?}", state.token);
    let greeting = state.stream.get_greeting();

    if state.write_pos == 0 {
        state.write_buffer[0..greeting.len()].copy_from_slice(&greeting);
    }

    match &mut state.stream {
        Stream::WebSocketTls(stream) => {
            state.phase = TestPhase::GreetingSendToken;
            stream
                .reregister(&poll, state.token, Interest::WRITABLE)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
            state.read_pos = 0;
            return Ok(1);
        }
        Stream::WebSocket(stream) => {
            state.phase = TestPhase::GreetingSendToken;
            stream
                .reregister(&poll, state.token, Interest::WRITABLE)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
            state.read_pos = 0;
            return Ok(1);
        }
        _ => loop {
            let n = state
                .stream
                .write(&state.write_buffer[state.write_pos..state.write_pos + greeting.len()])?;
            state.write_pos += n;
            if state.write_pos == state.stream.get_greeting().len() {
                state
                    .stream
                    .reregister(&poll, state.token, Interest::READABLE)?;
                state.phase = TestPhase::GreetingReceiveGreeting;
                state.write_pos = 0;
                state.read_pos = 0;
                return Ok(1);
            }
        },
    }
}

pub fn handle_greeting_send_token(
    poll: &Poll,
    state: &mut MeasurementState,
) -> Result<usize, std::io::Error> {
    debug!("handle_greeting_send_token token {:?}", state.token);
    let s = format!("TOKEN {}\n", state.token.0);

    if state.write_pos == 0 {
        debug!("[handle_greeting_send_token] Writing token command");
        //TODO send uuid
        state.write_buffer[state.write_pos..state.write_pos + s.len()]
            .copy_from_slice(s.as_bytes());
    }
    loop {
        let n = state
            .stream
            .write(&state.write_buffer[state.write_pos..state.write_pos + s.len()])?;
        state.write_pos += n;
        if state.write_pos == s.len() {
            state.write_pos = 0;
            state.read_pos = 0;
            state
                .stream
                .reregister(&poll, state.token, Interest::READABLE)?;
            state.phase = TestPhase::GreetingReceiveResponse;
            return Ok(n);
        }
        state
            .stream
            .reregister(&poll, state.token, Interest::WRITABLE)?;
    }
}

pub fn handle_greeting_receive_greeting(
    poll: &Poll,
    state: &mut MeasurementState,
) -> Result<usize, std::io::Error> {
    debug!("handle_greeting_receive_greeting token {:?}", state.token);
    loop {
        let n = state
            .stream
            .read(&mut state.read_buffer[state.read_pos..])?;
        state.read_pos += n;
        let end = b"ACCEPT TOKEN QUIT\n";
        let s =
            String::from_utf8_lossy(&state.read_buffer[state.read_pos - end.len()..state.read_pos]);
        if n > 0 && state.read_buffer[state.read_pos - end.len()..state.read_pos] == *end {
            state.phase = TestPhase::GreetingSendToken;
            state.read_pos = 0;
            state
                .stream
                .reregister(poll, state.token, Interest::WRITABLE)?;
            return Ok(n);
        }
    }
}

pub fn handle_greeting_receive_response(
    poll: &Poll,
    state: &mut MeasurementState,
) -> Result<usize, std::io::Error> {
    debug!("handle_greeting_receive_response token {:?}", state.token);
    loop {
        let n = state
            .stream
            .read(&mut state.read_buffer[state.read_pos..])?;
        state.read_pos += n;
        let end = b"ACCEPT GETCHUNKS GETTIME PUT PUTNORESULT PING QUIT\n";
        if n > 0
            && state.read_pos >= end.len()
            && state.read_buffer[state.read_pos - end.len()..state.read_pos] == *end
        {
            state.phase = TestPhase::GreetingCompleted;
            state
                .stream
                .reregister(poll, state.token, Interest::WRITABLE)?;
            state.read_pos = 0;
            return Ok(n);
        }
    }
}
