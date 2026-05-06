use log::{debug, info};
use mio::{Interest, Poll};
use std::io;
use std::net::UdpSocket;

use crate::client::state::{MeasurementState, TestPhase};
use crate::udp::payload::{rtts_from_json, UdpPayload, FLAG_HOLE_PUNCH};
use crate::udp::socket::{run_client_udp_in, run_client_udp_out};
use crate::udp::{
    DEFAULT_UDP_IN_NUM_PACKETS, DEFAULT_UDP_OUT_NUM_PACKETS, DEFAULT_UDP_DELAY_NS,
    DEFAULT_UDP_TMAX_NS,
};

// → GET UDPPORT\n
pub fn handle_udp_send_get_port(poll: &Poll, state: &mut MeasurementState) -> io::Result<usize> {
    debug!("handle_udp_send_get_port");
    let command = b"GET UDPPORT\n";
    if state.write_pos == 0 {
        state.write_buffer[..command.len()].copy_from_slice(command);
    }
    let len = command.len();
    loop {
        let n = state.stream.write(&state.write_buffer[state.write_pos..len])?;
        state.write_pos += n;
        if state.write_pos == len {
            state.write_pos = 0;
            state.read_pos = 0;
            state.phase = TestPhase::UdpReceivePort;
            state.stream.reregister(poll, state.token, Interest::READABLE)?;
            return Ok(n);
        }
    }
}

// ← <port>\n
pub fn handle_udp_receive_port(poll: &Poll, state: &mut MeasurementState) -> io::Result<usize> {
    debug!("handle_udp_receive_port");
    loop {
        let n = state.stream.read(&mut state.read_buffer[state.read_pos..])?;
        if n == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "EOF"));
        }
        state.read_pos += n;
        if state.read_buffer[..state.read_pos].contains(&b'\n') {
            let s = String::from_utf8_lossy(&state.read_buffer[..state.read_pos]);
            // Server may have buffered ACCEPT GETCHUNKS... before the port — find the port line
            let port = s.lines()
                .filter_map(|l| l.trim().parse::<u16>().ok())
                .find(|&p| p > 0);
            if let Some(port) = port {
                state.udp_out_port = Some(port);
                state.server_udp_port = port; // update in case server returned different port
                state.read_pos = 0;
                info!("UDP OUT port: {}", port);
                state.phase = TestPhase::UdpSendTestOut;
                state.stream.reregister(poll, state.token, Interest::WRITABLE)?;
                return Ok(n);
            }
            // No valid port yet — wait for more data
        }
    }
}

fn uuid_to_hex(uuid: &[u8; 16]) -> String {
    uuid.iter().map(|b| format!("{:02x}", b)).collect()
}

// → UDPTEST OUT <port> <n> <uuid_hex>\n
pub fn handle_udp_send_test_out(poll: &Poll, state: &mut MeasurementState) -> io::Result<usize> {
    debug!("handle_udp_send_test_out");
    // Generate UUID once and store — used both in the TCP command and the UDP packets
    if state.udp_out_uuid.is_none() {
        let mut uuid = [0u8; 16];
        for b in uuid.iter_mut() { *b = fastrand::u8(..); }
        state.udp_out_uuid = Some(uuid);
    }
    let port = state.udp_out_port.unwrap_or(0);
    let uuid_hex = uuid_to_hex(state.udp_out_uuid.as_ref().unwrap());
    let command = format!("UDPTEST OUT {} {} {}\n", port, DEFAULT_UDP_OUT_NUM_PACKETS, uuid_hex);
    if state.write_pos == 0 {
        let bytes = command.as_bytes();
        state.write_buffer[..bytes.len()].copy_from_slice(bytes);
    }
    let len = command.len();
    loop {
        let n = state.stream.write(&state.write_buffer[state.write_pos..len])?;
        state.write_pos += n;
        if state.write_pos == len {
            state.write_pos = 0;
            state.read_pos = 0;
            state.phase = TestPhase::UdpReceiveOkOut;
            state.stream.reregister(poll, state.token, Interest::READABLE)?;
            return Ok(n);
        }
    }
}

// ← OK\n  → run UDP OUT exchange synchronously
pub fn handle_udp_receive_ok_out(poll: &Poll, state: &mut MeasurementState) -> io::Result<usize> {
    debug!("handle_udp_receive_ok_out");
    loop {
        let n = state.stream.read(&mut state.read_buffer[state.read_pos..])?;
        if n == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "EOF"));
        }
        state.read_pos += n;
        if state.read_buffer[..state.read_pos].contains(&b'\n') {
            let s = String::from_utf8_lossy(&state.read_buffer[..state.read_pos]);
            // Server may have buffered ACCEPT GETCHUNKS... before OK — find the OK line
            if s.lines().find(|l| l.trim().starts_with("OK")).is_none() {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "Expected OK for UDPTEST OUT"));
            }
            state.read_pos = 0;

            // Run UDP OUT exchange synchronously, using the UUID already sent in UDPTEST OUT
            let server_ip = state.server_addr.ip();
            let port = state.udp_out_port.unwrap_or(0);
            let uuid = state.udp_out_uuid.unwrap_or([0u8; 16]);

            info!("Starting UDP OUT: {} packets → {}:{} uuid={}", DEFAULT_UDP_OUT_NUM_PACKETS, server_ip, port, uuid_to_hex(&uuid));
            let result = run_client_udp_out(
                server_ip,
                port,
                DEFAULT_UDP_OUT_NUM_PACKETS,
                DEFAULT_UDP_DELAY_NS,
                DEFAULT_UDP_TMAX_NS,
                uuid,
            );
            info!(
                "UDP OUT done: received={} loss={}% burst={}",
                result.received_packets, result.packet_loss_rate, result.max_burst_loss
            );
            state.udp_result_out = Some(result);

            state.phase = TestPhase::UdpSendGetResultOut;
            state.stream.reregister(poll, state.token, Interest::WRITABLE)?;
            return Ok(n);
        }
    }
}

// → GET UDPRESULT OUT <port>\n
pub fn handle_udp_send_get_result_out(
    poll: &Poll,
    state: &mut MeasurementState,
) -> io::Result<usize> {
    debug!("handle_udp_send_get_result_out");
    let port = state.udp_out_port.unwrap_or(0);
    let command = format!("GET UDPRESULT OUT {}\n", port);
    if state.write_pos == 0 {
        let bytes = command.as_bytes();
        state.write_buffer[..bytes.len()].copy_from_slice(bytes);
    }
    let len = command.len();
    loop {
        let n = state.stream.write(&state.write_buffer[state.write_pos..len])?;
        state.write_pos += n;
        if state.write_pos == len {
            state.write_pos = 0;
            state.read_pos = 0;
            state.phase = TestPhase::UdpReceiveResultOut;
            state.stream.reregister(poll, state.token, Interest::READABLE)?;
            return Ok(n);
        }
    }
}

// ← RCV <received> <port>\n
pub fn handle_udp_receive_result_out(
    poll: &Poll,
    state: &mut MeasurementState,
) -> io::Result<usize> {
    debug!("handle_udp_receive_result_out");
    loop {
        let n = state.stream.read(&mut state.read_buffer[state.read_pos..])?;
        if n == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "EOF"));
        }
        state.read_pos += n;
        if state.read_buffer[..state.read_pos].contains(&b'\n') {
            let s = String::from_utf8_lossy(&state.read_buffer[..state.read_pos]);
            let s = s.trim();
            // Parse "RCV <received> <port>"
            if let Some(rest) = s.strip_prefix("RCV ") {
                let parts: Vec<&str> = rest.split_whitespace().collect();
                if parts.len() >= 1 {
                    let server_received: u32 = parts[0].parse().unwrap_or(0);
                    state.udp_server_received_out = Some(server_received);
                    info!("UDP OUT server received: {}", server_received);
                }
            }
            state.read_pos = 0;
            state.phase = TestPhase::UdpSendTestIn;
            state.stream.reregister(poll, state.token, Interest::WRITABLE)?;
            return Ok(n);
        }
    }
}

// → UDPTEST IN <in_port> <n> <uuid_hex>\n  + hole punch + run UDP IN receive
pub fn handle_udp_send_test_in(poll: &Poll, state: &mut MeasurementState) -> io::Result<usize> {
    debug!("handle_udp_send_test_in");

    // Bind local socket first so we know the port before sending the command
    if state.udp_in_socket.is_none() {
        let sock = UdpSocket::bind("0.0.0.0:0").map_err(|e| {
            io::Error::new(io::ErrorKind::Other, format!("UDP IN bind: {}", e))
        })?;
        sock.set_read_timeout(Some(std::time::Duration::from_millis(100))).ok();
        let port = sock.local_addr()?.port();
        state.udp_in_port = Some(port);

        // Generate UUID for this IN test (used for hole punch routing on server)
        let mut in_uuid = [0u8; 16];
        for b in in_uuid.iter_mut() { *b = fastrand::u8(..); }

        // Hole punch: send from in_socket to server's UDP port to open NAT mapping
        let server_addr = std::net::SocketAddr::new(
            state.server_addr.ip(),
            state.server_udp_port,
        );
        let hole_punch = UdpPayload {
            communication_flag: FLAG_HOLE_PUNCH,
            packet_number:      0,
            uuid:               in_uuid,
            timestamp_ns:       0,
        };
        sock.send_to(&hole_punch.to_bytes(), server_addr).ok();
        info!("UDP IN: hole punch sent to {} from port {}", server_addr, port);

        // Store UUID in socket slot (reuse existing field)
        state.udp_in_uuid   = Some(in_uuid);
        state.udp_in_socket = Some(sock);
    }

    let in_port  = state.udp_in_port.unwrap_or(0);
    let in_uuid  = state.udp_in_uuid.unwrap_or([0u8; 16]);
    let uuid_hex = uuid_to_hex(&in_uuid);
    let command  = format!("UDPTEST IN {} {} {}\n", in_port, DEFAULT_UDP_IN_NUM_PACKETS, uuid_hex);

    if state.write_pos == 0 {
        let bytes = command.as_bytes();
        state.write_buffer[..bytes.len()].copy_from_slice(bytes);
    }

    let len = command.len();
    loop {
        let n = state.stream.write(&state.write_buffer[state.write_pos..len])?;
        state.write_pos += n;
        if state.write_pos == len {
            state.write_pos = 0;
            state.read_pos = 0;

            // Run UDP IN receive loop synchronously
            if let Some(sock) = state.udp_in_socket.take() {
                info!(
                    "Starting UDP IN: expecting {} packets on port {}",
                    DEFAULT_UDP_IN_NUM_PACKETS, in_port
                );
                let result = run_client_udp_in(&sock, DEFAULT_UDP_IN_NUM_PACKETS, DEFAULT_UDP_TMAX_NS);
                info!(
                    "UDP IN done: received={} loss={}%",
                    result.received_packets, result.packet_loss_rate
                );
                state.udp_result_in = Some(result);
            }

            state.phase = TestPhase::UdpSendGetResultIn;
            state.stream.reregister(poll, state.token, Interest::WRITABLE)?;
            return Ok(n);
        }
    }
}

// → GET UDPRESULT IN <in_port>\n
pub fn handle_udp_send_get_result_in(
    poll: &Poll,
    state: &mut MeasurementState,
) -> io::Result<usize> {
    debug!("handle_udp_send_get_result_in");
    let port = state.udp_in_port.unwrap_or(0);
    let command = format!("GET UDPRESULT IN {}\n", port);
    if state.write_pos == 0 {
        let bytes = command.as_bytes();
        state.write_buffer[..bytes.len()].copy_from_slice(bytes);
    }
    let len = command.len();
    loop {
        let n = state.stream.write(&state.write_buffer[state.write_pos..len])?;
        state.write_pos += n;
        if state.write_pos == len {
            state.write_pos = 0;
            state.read_pos = 0;
            state.phase = TestPhase::UdpReceiveResultIn;
            state.stream.reregister(poll, state.token, Interest::READABLE)?;
            return Ok(n);
        }
    }
}

// ← RCV <received> <port> <json_rtts>\n
pub fn handle_udp_receive_result_in(
    poll: &Poll,
    state: &mut MeasurementState,
) -> io::Result<usize> {
    debug!("handle_udp_receive_result_in");
    loop {
        let n = state.stream.read(&mut state.read_buffer[state.read_pos..])?;
        if n == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "EOF"));
        }
        state.read_pos += n;
        if state.read_buffer[..state.read_pos].contains(&b'\n') {
            let s = String::from_utf8_lossy(&state.read_buffer[..state.read_pos]);
            let s = s.trim();
            // Parse "RCV <received> <port> [<json>]"
            if let Some(rest) = s.strip_prefix("RCV ") {
                let parts: Vec<&str> = rest.splitn(3, ' ').collect();
                if parts.len() >= 1 {
                    let server_received: u32 = parts[0].parse().unwrap_or(0);
                    info!("UDP IN server echoes: {}", server_received);
                }
                // RTTs from server (parts[2] is optional JSON)
                if parts.len() >= 3 {
                    let rtts = rtts_from_json(parts[2]);
                    if !rtts.is_empty() {
                        if let Some(ref mut res) = state.udp_result_in {
                            let avg = rtts.values().sum::<u64>() / rtts.len() as u64;
                            let min = rtts.values().copied().min();
                            let max = rtts.values().copied().max();
                            res.rtt_avg_ns = Some(avg);
                            res.rtt_min_ns = min;
                            res.rtt_max_ns = max;
                            res.rtts_ns = rtts;
                        }
                    }
                }
            }
            state.read_pos = 0;
            state.phase = TestPhase::UdpCompleted;
            state.stream.reregister(poll, state.token, Interest::WRITABLE)?;
            return Ok(n);
        }
    }
}
