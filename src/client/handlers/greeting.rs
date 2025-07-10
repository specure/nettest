use crate::client::{
    handlers::BasicHandler, state::TestPhase, MeasurementState,
};
use crate::stream::stream::Stream;
use anyhow::Result;
use log::{debug, trace};
use mio::{Interest, Poll, Token};
use std::io;

pub struct GreetingHandler {
}

impl GreetingHandler {
    pub fn new(token: Token) -> Result<Self> {
        Ok(Self {
        })
    }
}

impl BasicHandler for GreetingHandler {
    fn on_read(&mut self, poll: &Poll, measurement_state: &mut MeasurementState) -> Result<()> {
        match measurement_state.phase {
            TestPhase::GreetingReceiveGreeting => {
                debug!("[on_read] GreetingReceiveGreeting");
                match handle_greeting_receive_greeting(poll, measurement_state) {
                    Ok(n) => {
                        return Ok(());
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        return Ok(());
                    }
                    Err(e) => {
                        return Err(anyhow::anyhow!("Error reading greeting: {}", e));
                    }
                }
            }
            TestPhase::GreetingReceiveResponse => {
                debug!("[on_read] GreetingReceiveResponse");
                match handle_greeting_receive_response(poll, measurement_state) {
                    Ok(n) => {
                        return Ok(());
                    }
                    Err(e) => {
                        return Err(anyhow::anyhow!("Error reading greeting: {}", e));
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        return Ok(());
                    }
                }
            }

            _ => {
                return Ok(());
            }
        }
    }

    fn on_write(&mut self, poll: &Poll, measurement_state: &mut MeasurementState) -> Result<()> {
        match measurement_state.phase {
            TestPhase::GreetingSendConnectionType => {
                debug!("[on_write] Sending connection type command");
                match handle_greeting_send_connection_type(poll, measurement_state) {
                    Ok(n) => {
                        return Ok(());
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        return Ok(());
                    }
                    Err(e) => {
                        return Err(anyhow::anyhow!("Error sending connection type: {}", e));
                    }
                }
            }
            TestPhase::GreetingSendToken => {
                debug!("[on_write] GreetingSendToken");
                match handle_greeting_send_token(poll, measurement_state) {
                    Ok(n) => {
                        return Ok(());
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        return Ok(());
                    }
                    Err(e) => {
                        return Err(anyhow::anyhow!("Error sending token: {}", e));
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }
}

fn handle_greeting_send_connection_type(
    poll: &Poll,
    state: &mut MeasurementState,
) -> Result<usize, std::io::Error> {
    debug!("[handle_greeting_send_connection_type] Sending connection type command");
    let greeting = state.stream.get_greeting();

    if state.write_pos == 0 {
        debug!("[on_write] Writing connection 1 type command");
        state.write_buffer[0..greeting.len()].copy_from_slice(&greeting);
    }

    match &mut state.stream {
        Stream::WebSocketTls(stream) => {
            debug!("[on_write] WebSocketTls greeting sent",);
            state.phase = TestPhase::GreetingSendToken;
            stream
                .reregister(&poll, state.token, Interest::WRITABLE)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
            state.read_pos = 0;
            return Ok(1);
        }
        Stream::WebSocket(stream) => {
            debug!("[on_write] WebSocket greeting sent",);

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

fn handle_greeting_send_token(
    poll: &Poll,
    state: &mut MeasurementState,
) -> Result<usize, std::io::Error> {
    debug!("[handle_greeting_send_token] Sending token command");
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
        // return Ok(n);
    }
}

pub fn handle_greeting_receive_greeting(
    poll: &Poll,
    state: &mut MeasurementState,
) -> Result<usize, std::io::Error> {
    trace!("handle_greeting_receive_greeting");
    loop {
        let n = state
            .stream
            .read(&mut state.read_buffer[state.read_pos..])?;
        debug!("[handle_greeting_receive_greeting] read {} bytes", n);
        state.read_pos += n;
        let end = b"ACCEPT TOKEN QUIT\n";
        let s =
            String::from_utf8_lossy(&state.read_buffer[state.read_pos - end.len()..state.read_pos]);
        debug!("[handle_greeting_receive_greeting] read {}", s);
        if n > 0 && state.read_buffer[state.read_pos - end.len()..state.read_pos] == *end {
            state.phase = TestPhase::GreetingSendToken;
            debug!(
                "Greeting received token: {}",
                String::from_utf8_lossy(&state.read_buffer)
            );
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
    trace!("handle_greeting_receive_response");
    loop {
        let n = state
            .stream
            .read(&mut state.read_buffer[state.read_pos..])?;
        state.read_pos += n;
        let end = b"ACCEPT GETCHUNKS GETTIME PUT PUTNORESULT PING QUIT\n";
        debug!("[handle_greeting_receive_response] read {}", String::from_utf8_lossy(&state.read_buffer));
        if n > 0 && state.read_pos >= end.len() && state.read_buffer[state.read_pos - end.len()..state.read_pos] == *end {
            state.phase = TestPhase::GreetingCompleted;
            state
                .stream
                .reregister(poll, state.token, Interest::WRITABLE)?;
            state.read_pos = 0;
            return Ok(n);
        }
    }
}
