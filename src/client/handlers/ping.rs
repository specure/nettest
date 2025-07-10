use crate::client::handlers::BasicHandler;
use crate::client::state::TestPhase;
use crate::client::utils::ACCEPT_GETCHUNKS_STRING;
use crate::client::{write_all_nb, MeasurementState};
use crate::stream::stream::Stream;
use anyhow::Result;
use bytes::BytesMut;
use log::debug;
use mio::{Interest, Poll, Token};
use std::io;
use std::time::Instant;

const MAX_PINGS: u32 = 200;
const PING_DURATION_NS: u64 = 1_000_000_000; // 1 second
const PONG_RESPONSE: &[u8] = b"PONG\n";

pub struct PingHandler {
}

impl PingHandler {
    pub fn new(token: Token) -> Result<Self> {
        Ok(Self {
        })
    }
    
}

impl BasicHandler for PingHandler {
    fn on_read(&mut self, poll: &Poll, measurement_state: &mut MeasurementState) -> Result<()> {
        match measurement_state.phase {
            TestPhase::PingReceivePong => match handle_receive_pong(poll, measurement_state) {
                Ok(n) => {
                    return Ok(());
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    return Ok(());
                }
                Err(e) => {
                    return Err(e.into());
                }
            },
            TestPhase::PingReceiveTime => {
                debug!("PingReceiveTime");
                match handle_receive_time(poll, measurement_state) {
                    Ok(n) => {
                        return Ok(());
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        return Ok(());
                    }
                    Err(e) => {
                        return Err(e.into());
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn on_write(&mut self, poll: &Poll, measurement_state: &mut MeasurementState) -> Result<()> {
        match measurement_state.phase {
            TestPhase::PingSendPing => {
                debug!("PingSendPing");
                match handle_send_ping(poll, measurement_state) {
                    Ok(n) => {
                        return Ok(());
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        return Ok(());
                    }
                    Err(e) => {
                        return Err(e.into());
                    }
                }
            }
            TestPhase::PingSendOk => match handle_send_ok(poll, measurement_state) {
                Ok(n) => {
                    return Ok(());
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    return Ok(());
                }
                Err(e) => {
                    return Err(e.into());
                }
            },
            _ => {}
        }
        Ok(())
    }
}

pub fn handle_send_ok(poll: &Poll, state: &mut MeasurementState) -> Result<usize, std::io::Error> {
    debug!("PingSendOk");
    if state.write_pos == 0 {
        state.write_buffer[0..b"OK\n".len()].copy_from_slice(b"OK\n");
    }
    loop {
        let n = state.stream.write(&state.write_buffer[state.write_pos..b"OK\n".len()])?;
        state.write_pos += n;
        if state.write_pos == b"OK\n".len() {
            state.write_pos = 0;
            state.read_pos = 0;
            state.phase = TestPhase::PingReceiveTime;
            state
                .stream
                .reregister(&poll, state.token, Interest::READABLE)?;
            return Ok(n);
        }
    }
}

pub fn handle_send_ping(
    poll: &Poll,
    state: &mut MeasurementState,
) -> Result<usize, std::io::Error> {
    debug!("PingSendPing");
    if state.write_pos == 0 {
        state.write_buffer[0..b"PING\n".len()].copy_from_slice(b"PING\n");
    }
    loop {
        let n = state.stream.write(
            &state.write_buffer[state.write_pos..b"PING\n".len()],
        )?;
        state.write_pos += n;
        if state.write_pos == b"PING\n".len() {
            if state.phase_start_time.is_none() {
                state.phase_start_time = Some(Instant::now());
            }
            state.phase = TestPhase::PingReceivePong;
            state
                .stream
                .reregister(&poll, state.token, Interest::READABLE)?;
            state.write_pos = 0;
            state.read_pos = 0;
            return Ok(n);
        }
    }
}

pub fn handle_receive_pong(
    poll: &Poll,
    state: &mut MeasurementState,
) -> Result<usize, std::io::Error> {
    debug!("PingReceivePong");
    loop {
        let n = state
            .stream
            .read(&mut state.read_buffer[state.read_pos..PONG_RESPONSE.len()])?;
        state.read_pos += n;
        if state.read_pos == PONG_RESPONSE.len() {
            state.read_pos = 0;
            state.phase = TestPhase::PingSendOk;
            state
                .stream
                .reregister(&poll, state.token, Interest::WRITABLE)?;
            return Ok(n);
        }
    }
}

pub fn handle_receive_time(
    poll: &Poll,
    state: &mut MeasurementState,
) -> Result<usize, std::io::Error> {
    debug!("PingReceiveTime");
    loop {
        debug!("PingReceiveTime: {}", String::from_utf8_lossy(&state.read_buffer));
        let n = state
            .stream
            .read(&mut state.read_buffer[state.read_pos..])?;
        state.read_pos += n;
        if state.read_pos >= ACCEPT_GETCHUNKS_STRING.len()
            && state.read_buffer[state.read_pos - ACCEPT_GETCHUNKS_STRING.len()..state.read_pos]
                == *ACCEPT_GETCHUNKS_STRING.as_bytes()
        {
            debug!("PingReceiveTime: {}", String::from_utf8_lossy(&state.read_buffer));
            let elapsed = state.phase_start_time.unwrap().elapsed();
            let buffer_str = String::from_utf8_lossy(&state.read_buffer);
            if let Some(time_start) = buffer_str.find("TIME ") {
                if let Some(time_end) = buffer_str[time_start..].find('\n') {
                    let time_str = &buffer_str[time_start + 5..time_start + time_end];

                    if let Ok(time_ns) = time_str.parse::<u64>() {
                        state.ping_times.push(time_ns);
                        let pings_sent = state.ping_times.len();

                        if elapsed.as_nanos() < PING_DURATION_NS as u128
                            && pings_sent < MAX_PINGS as usize
                        {
                            state.phase = TestPhase::PingSendPing;
                            state
                                .stream
                                .reregister(&poll, state.token, Interest::WRITABLE)?;
                            state.read_pos = 0;
                            return Ok(n);
                        } else {
                            // Calculate final median latency
                            if let Some(median) = get_median_latency(state) {
                                state.time_result = Some(median);
                                state.ping_median = Some(median);
                            }
                            state.phase = TestPhase::PingCompleted;
                            state
                                .stream
                                .reregister(&poll, state.token, Interest::WRITABLE)?;
                            state.read_pos = 0;
                            return Ok(n);
                        }
                    }
                }
            }
        }
    }
}

pub fn get_median_latency(state: &mut MeasurementState) -> Option<u64> {
    let mut sorted_times = state.ping_times.clone();
    sorted_times.sort_unstable();

    let mid = sorted_times.len() / 2;
    if sorted_times.len() % 2 == 0 {
        Some((sorted_times[mid - 1] + sorted_times[mid]) / 2)
    } else {
        Some(sorted_times[mid])
    }
}