use log::{info, warn};
use mio::{Interest, Poll};
use std::io;
use std::thread;
use std::time::Duration;

use crate::mioserver::{server::TestState, ServerTestPhase};
use crate::udp::payload::rtts_to_json;
use crate::udp::result::{UdpServerInResult, UdpServerOutResult};
use crate::udp::socket::{start_server_udp_in, start_server_udp_out};

// GET UDPPORT — bind a UDP socket, respond with the assigned port
pub fn handle_udp_send_port(poll: &Poll, state: &mut TestState) -> io::Result<usize> {
    if state.udp_out_socket.is_none() {
        // Bind to get OS-assigned port
        let sock = std::net::UdpSocket::bind("0.0.0.0:0").map_err(|e| {
            io::Error::new(io::ErrorKind::Other, format!("UDP bind: {}", e))
        })?;
        state.udp_out_port = Some(sock.local_addr()?.port());
        state.udp_out_socket = Some(sock);
    }

    let port = state.udp_out_port.unwrap_or(0);
    let response = format!("{}\n", port);

    if state.write_pos == 0 {
        let bytes = response.as_bytes();
        state.write_buffer[..bytes.len()].copy_from_slice(bytes);
    }

    let len = response.len();
    loop {
        let n = state.stream.write(&state.write_buffer[state.write_pos..len])?;
        state.write_pos += n;
        if state.write_pos == len {
            info!("UDP port allocated: {}", port);
            state.write_pos = 0;
            state.read_pos = 0;
            state.measurement_state = ServerTestPhase::AcceptCommandReceive;
            state.stream.reregister(poll, state.token, Interest::READABLE)?;
            return Ok(n);
        }
    }
}

// UDPTEST OUT — respond OK, start receive thread
pub fn handle_udp_send_ok_out(poll: &Poll, state: &mut TestState) -> io::Result<usize> {
    let response = b"OK\n";

    if state.write_pos == 0 {
        state.write_buffer[..response.len()].copy_from_slice(response);
    }

    let len = response.len();
    loop {
        let n = state.stream.write(&state.write_buffer[state.write_pos..len])?;
        state.write_pos += n;
        if state.write_pos == len {
            // Start UDP receive thread using the pre-bound socket
            if let Some(socket) = state.udp_out_socket.take() {
                let count = state.udp_out_num_packets.unwrap_or(50);
                let result = start_server_udp_out(socket, count);
                state.udp_out_result = Some(result);
            } else {
                warn!("UDP OUT: no pre-bound socket available");
            }
            state.write_pos = 0;
            state.read_pos = 0;
            state.measurement_state = ServerTestPhase::AcceptCommandReceive;
            state.stream.reregister(poll, state.token, Interest::READABLE)?;
            return Ok(n);
        }
    }
}

// GET UDPRESULT OUT — wait for thread, respond RCV <n> <port>
pub fn handle_udp_send_result_out(poll: &Poll, state: &mut TestState) -> io::Result<usize> {
    let result_arc = match &state.udp_out_result {
        Some(a) => a.clone(),
        None => {
            warn!("UDP OUT result store missing");
            let port = state.udp_out_port.unwrap_or(0);
            let response = format!("RCV 0 {}\n", port);
            return write_response(poll, state, &response, ServerTestPhase::AcceptCommandReceive);
        }
    };

    let result: UdpServerOutResult = loop {
        let guard = result_arc.lock().unwrap();
        if let Some(r) = guard.clone() {
            break r;
        }
        drop(guard);
        thread::sleep(Duration::from_millis(10));
    };

    let response = format!("RCV {} {}\n", result.received, result.port);
    info!("UDP OUT result: received={} port={}", result.received, result.port);
    state.udp_out_result = None;
    write_response(poll, state, &response, ServerTestPhase::AcceptCommandReceive)
}

// UDPTEST IN — start send+receive thread (no TCP response, server just sends UDP)
pub fn handle_udp_start_in(state: &mut TestState) {
    let client_ip = state
        .client_addr
        .map(|a| a.ip())
        .unwrap_or(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST));
    let client_port = state.udp_in_client_port.unwrap_or(0);
    let count = state.udp_in_num_packets.unwrap_or(50);

    let result = start_server_udp_in(client_ip, client_port, count);
    state.udp_in_result = Some(result);
    info!("UDP IN: started send thread → {}:{}", client_ip, client_port);
}

// GET UDPRESULT IN — wait for thread, respond RCV <n> <port> <json_rtts>
pub fn handle_udp_send_result_in(poll: &Poll, state: &mut TestState) -> io::Result<usize> {
    let result_arc = match &state.udp_in_result {
        Some(a) => a.clone(),
        None => {
            warn!("UDP IN result store missing");
            let response = "RCV 0 0 {}\n".to_string();
            return write_response(poll, state, &response, ServerTestPhase::AcceptCommandReceive);
        }
    };

    let result: UdpServerInResult = loop {
        let guard = result_arc.lock().unwrap();
        if let Some(r) = guard.clone() {
            break r;
        }
        drop(guard);
        thread::sleep(Duration::from_millis(10));
    };

    let json = rtts_to_json(&result.rtts);
    let response = format!("RCV {} {} {}\n", result.received, result.port, json);
    info!("UDP IN result: received={} port={}", result.received, result.port);
    state.udp_in_result = None;
    write_response(poll, state, &response, ServerTestPhase::AcceptCommandReceive)
}

fn write_response(
    poll: &Poll,
    state: &mut TestState,
    response: &str,
    next: ServerTestPhase,
) -> io::Result<usize> {
    if state.write_pos == 0 {
        let bytes = response.as_bytes();
        if bytes.len() <= state.write_buffer.len() {
            state.write_buffer[..bytes.len()].copy_from_slice(bytes);
        }
    }
    let len = response.len();
    loop {
        let n = state.stream.write(&state.write_buffer[state.write_pos..len])?;
        state.write_pos += n;
        if state.write_pos == len {
            state.write_pos = 0;
            state.read_pos = 0;
            state.measurement_state = next;
            state.stream.reregister(poll, state.token, Interest::READABLE)?;
            return Ok(n);
        }
    }
}
