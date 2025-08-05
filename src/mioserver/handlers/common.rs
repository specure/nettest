use log::{debug, trace};
use mio::{Interest, Poll};
use std::io;

use crate::{
    config::constants::{MAX_CHUNK_SIZE, MIN_CHUNK_SIZE},
    mioserver::{server::TestState, ServerTestPhase},
};

pub fn handle_main_command_send(poll: &Poll, state: &mut TestState) -> io::Result<usize> {
    debug!("handle_get_put_ping_quit_send");
    let command = b"ACCEPT GETCHUNKS GETTIME PUT PUTNORESULT PING QUIT\n";
    if state.write_pos == 0 {
        trace!("Wrote handle_get_put_ping_quit_send {}", command.len());
        state.write_buffer[0..command.len()].copy_from_slice(command);
    }
    let command_len = command.len();
    loop {
        let n = state
            .stream
            .write(&state.write_buffer[state.write_pos..command_len])?;
        trace!("Wrote handle_get_put_ping_quit_send {} ", n);
        state.write_pos += n;
        if state.write_pos == command_len {
            state.write_pos = 0;
            state.read_pos = 0;
            state.measurement_state = ServerTestPhase::AcceptCommandReceive;
            state
                .stream
                .reregister(poll, state.token, Interest::READABLE)?;
            return Ok(n);
        }
    }
}

pub fn handle_main_command_receive(poll: &Poll, state: &mut TestState) -> io::Result<usize> {
    debug!("handle_receive_command");
    loop {
        trace!("handle_receive_command read_pos: {}", state.read_pos);
        let n = state
            .stream
            .read(&mut state.read_buffer[state.read_pos..])?;
        state.read_pos += n;
        if n == 0 {
            return Ok(n);
        }

        if state.read_buffer[state.read_pos - 1..state.read_pos] == [b'\n'] {
            let command_str = String::from_utf8_lossy(&state.read_buffer[..state.read_pos]);

            debug!("command_str: {}", command_str);

            if command_str.contains("GETCHUNKS") {
                let commands: Vec<&str> = command_str.split_terminator('\n').collect();

                let command_str = commands[commands.len() - 1];

                let parts: Vec<&str> = command_str[9..].trim().split_whitespace().collect();

                let num_chunks = match parts[0].parse::<usize>() {
                    Ok(n) => n,
                    Err(_) => {
                        return Err(io::Error::new(io::ErrorKind::Other, "Invalid command"));
                    }
                };

                let chunk_size = if parts.len() == 1 {
                    MIN_CHUNK_SIZE
                } else {
                    match parts[1].parse::<usize>() {
                        Ok(size) if size >= MIN_CHUNK_SIZE && size <= MAX_CHUNK_SIZE => size,
                        _ => {
                            return Err(io::Error::new(io::ErrorKind::Other, "Invalid command"));
                        }
                    }
                };

                state.num_chunks = num_chunks;
                state.chunk_size = chunk_size;
                state.measurement_state = ServerTestPhase::GetChunkSendChunk;
                state.read_pos = 0;

                state
                    .stream
                    .reregister(poll, state.token, Interest::WRITABLE)?;
                return Ok(n);
            }

            if command_str.starts_with("PING\n") {
                state.read_pos = 0;

                state.measurement_state = ServerTestPhase::PongSend;
                state
                    .stream
                    .reregister(poll, state.token, Interest::WRITABLE)?;
                return Ok(n);
            }

            if command_str.starts_with("GETTIME") {
                let parts: Vec<&str> = command_str[7..].trim().split_whitespace().collect();

                // Parse duration using strtoul-like parsing
                let duration = match parts[0].parse::<u64>() {
                    Ok(d) => d,
                    Err(_) => {
                        return Err(io::Error::new(io::ErrorKind::Other, "Invalid command"));
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
                            return Err(io::Error::new(io::ErrorKind::Other, "Invalid command"));
                        }
                    }
                };

                state.chunk_size = chunk_size;
                state.read_pos = 0;
                state.measurement_state = ServerTestPhase::GetTimeSendChunk;

                state
                    .stream
                    .reregister(poll, state.token, Interest::WRITABLE)?;
                return Ok(n);
            }

            if command_str.starts_with("PUTNORESULT") {
                let parts: Vec<&str> = command_str.split_whitespace().collect();

                if parts.len() > 2 {
                    return Err(io::Error::new(io::ErrorKind::Other, "Invalid command"));
                }


                if parts.len() == 2 {
                    match parts[1].parse::<usize>() {
                        Ok(size) if size >= MIN_CHUNK_SIZE && size <= MAX_CHUNK_SIZE => {
                            state.chunk_size = size;
                        }
                        _ => {
                            trace!("Invalid chunk size");
                        }
                    }
                } else {
                    state.chunk_size = MIN_CHUNK_SIZE;
                }
                state.read_pos = 0;
                state.measurement_state = ServerTestPhase::PutNoResultSendOk;
                state
                    .stream
                    .reregister(poll, state.token, Interest::WRITABLE)?;
                return Ok(n);
            }

            if command_str.starts_with("PUTTIMERESULT") {
                state.read_pos = 0;
                state.measurement_state = ServerTestPhase::PutTimeResultSendOk;
                let parts: Vec<&str> = command_str.split_whitespace().collect();


                match parts[1].parse::<usize>() {
                    Ok(size) if size >= MIN_CHUNK_SIZE && size <= MAX_CHUNK_SIZE => {
                        state.chunk_size = size;
                    }
                    _ => {
                        state.chunk_size = MIN_CHUNK_SIZE;
                    }
                }


                state
                    .stream
                    .reregister(poll, state.token, Interest::WRITABLE)?;

                return Ok(n);
            }

            if command_str.starts_with("PUT") {
                let parts: Vec<&str> = command_str.split_whitespace().collect();

                if parts.len() > 2 {
                    return Err(io::Error::new(io::ErrorKind::Other, "Invalid command"));
                }

                if parts.len() == 2 {
                    match parts[1].parse::<usize>() {
                        Ok(size) if size >= MIN_CHUNK_SIZE && size <= MAX_CHUNK_SIZE => {
                            state.chunk_size = size;
                        }
                        _ => {
                            state.chunk_size = MIN_CHUNK_SIZE;
                        }
                    }
                } else {
                    state.chunk_size = MIN_CHUNK_SIZE;
                }
                state.read_pos = 0;
                state.measurement_state = ServerTestPhase::PutSendOk;
                state
                    .stream
                    .reregister(poll, state.token, Interest::WRITABLE)?;
                return Ok(n);
            }

            state.measurement_state = ServerTestPhase::AcceptCommandReceive;
            return Ok(n);
        }
    }
}
