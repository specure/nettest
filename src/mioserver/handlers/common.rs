use log::debug;
use mio::{Interest, Poll};
use std::io;

use crate::{
    config::constants::{MAX_CHUNK_SIZE, MIN_CHUNK_SIZE},
    mioserver::{server::TestState, ServerTestPhase},
};

pub fn handle_main_command_send(poll: &Poll, state: &mut TestState) -> io::Result<()> {
    debug!("handle_accept_token_quit_send");
    let command = b"ACCEPT GETCHUNKS GETTIME PUT PUTNORESULT PING QUIT\n";
    if state.write_pos == 0 {
        state.write_buffer[0..command.len()].copy_from_slice(command);
    }
    let command_len = command.len();
    loop {
        match state.stream.write(&state.write_buffer[state.write_pos..command_len]) {
            Ok(n) if n > 0 => {
                state.write_pos += n;
                debug!("n: {}", n);
                if state.write_pos == command_len {
                    state.write_pos = 0;
                    state.measurement_state = ServerTestPhase::AcceptCommandReceive;
                    if let Err(e) = state
                        .stream
                        .reregister(poll, state.token, Interest::READABLE)
                    {
                        return Err(io::Error::new(io::ErrorKind::Other, e));
                    }
                    return Ok(());
                }
            }
            Ok(_) => {
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

pub fn handle_main_command_receive(poll: &Poll, state: &mut TestState) -> io::Result<()> {
    debug!("handle_receive_command");
    loop {
        match state.stream.read(&mut state.read_buffer[state.read_pos..]) {
            Ok(n) if n > 0 => {

                state.read_pos += n;

                if state.read_buffer[state.read_pos - 1..state.read_pos] == [b'\n'] {
                    let command_str = String::from_utf8_lossy(&state.read_buffer[..state.read_pos]);

                    if command_str.starts_with("GETCHUNKS") {
                        let parts: Vec<&str> = command_str[9..].trim().split_whitespace().collect();

                        if parts.len() != 1 && parts.len() != 2 {
                            //TODO: send error
                            return Err(io::Error::new(io::ErrorKind::Other, "Invalid command"));
                        }

                        let num_chunks = match parts[0].parse::<usize>() {
                            Ok(n) => n,
                            Err(_) => {
                                return Err(io::Error::new(
                                    io::ErrorKind::Other,
                                    "Invalid command",
                                ));
                            }
                        };

                        let chunk_size = if parts.len() == 1 {
                            MIN_CHUNK_SIZE
                        } else {
                            match parts[1].parse::<usize>() {
                                Ok(size) if size >= MIN_CHUNK_SIZE && size <= MAX_CHUNK_SIZE => {
                                    size
                                }
                                _ => {
                                    return Err(io::Error::new(
                                        io::ErrorKind::Other,
                                        "Invalid command",
                                    ));
                                }
                            }
                        };

                        state.num_chunks = num_chunks;
                        state.chunk_size = chunk_size;
                        state.measurement_state = ServerTestPhase::GetChunkSendChunk;
                        state.read_pos = 0;

                        if let Err(e) =
                            state
                                .stream
                                .reregister(poll, state.token, Interest::WRITABLE)
                        {
                            return Err(io::Error::new(io::ErrorKind::Other, e));
                        }
                        return Ok(());
                    }

                    if command_str.starts_with("PING\n") {
                        state.read_pos = 0;

                        state.measurement_state = ServerTestPhase::PongSend;
                        if let Err(e) =
                            state
                                .stream
                                .reregister(poll, state.token, Interest::WRITABLE)
                        {
                            return Err(io::Error::new(io::ErrorKind::Other, e));
                        }
                        return Ok(());
                    }

                    if command_str.starts_with("GETTIME") {
                        let parts: Vec<&str> = command_str[7..].trim().split_whitespace().collect();

                        // Parse duration using strtoul-like parsing
                        let duration = match parts[0].parse::<u64>() {
                            Ok(d) => d,
                            Err(_) => {
                                return Err(io::Error::new(
                                    io::ErrorKind::Other,
                                    "Invalid command",
                                ));
                            }
                        };

                        state.duration = duration;

                        let chunk_size = if parts.len() == 1 {
                            MIN_CHUNK_SIZE
                        } else {
                            match parts[1].parse::<usize>() {
                                Ok(size) => {
                                    if size < MIN_CHUNK_SIZE || size > MAX_CHUNK_SIZE {
                                        return Err(io::Error::new(
                                            io::ErrorKind::Other,
                                            "Invalid command",
                                        ));
                                    }
                                    size
                                }
                                Err(_) => {
                                    return Err(io::Error::new(
                                        io::ErrorKind::Other,
                                        "Invalid command",
                                    ));
                                }
                            }
                        };

                        state.chunk_size = chunk_size;
                        state.read_pos = 0;
                        state.measurement_state = ServerTestPhase::GetTimeSendChunk;

                        if let Err(e) =
                            state
                                .stream
                                .reregister(poll, state.token, Interest::WRITABLE)
                        {
                            return Err(io::Error::new(io::ErrorKind::Other, e));
                        }
                        return Ok(());
                    }

                    state.measurement_state = ServerTestPhase::AcceptCommandReceive;
                    return Ok(());
                }
            }
            Ok(_) => {
                return Ok(());
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                return Ok(());
            }
            Err(e) => {
                debug!("Error: {:?}", e);
                return Err(e);
            }
        }
    }
}
