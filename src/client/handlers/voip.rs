use log::{debug, info};
use mio::{Interest, Poll};
use std::io;
use std::net::UdpSocket;

use crate::client::state::{MeasurementState, TestPhase};
use crate::voip::udp::run_client_udp;
use crate::voip::{
    VoipParams, DEFAULT_BITS_PER_SAMPLE, DEFAULT_BUFFER_NS, DEFAULT_DELAY_MS,
    DEFAULT_DURATION_MS, DEFAULT_PAYLOAD_TYPE, DEFAULT_SAMPLE_RATE, DEFAULT_VOIP_UDP_PORT,
};

pub fn handle_voip_send_command(
    poll: &Poll,
    state: &mut MeasurementState,
) -> io::Result<usize> {
    debug!("handle_voip_send_command");

    if state.write_pos == 0 {
        // Bind a dynamic UDP port for incoming RTP from server
        let udp_sock = UdpSocket::bind("0.0.0.0:0")
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        let in_port = udp_sock.local_addr()?.port();
        // Socket is used only to reserve the port; actual usage is in the UDP thread
        // Store port in params
        let initial_seq = fastrand::u16(..10000);

        let params = VoipParams {
            out_port: DEFAULT_VOIP_UDP_PORT,
            in_port,
            sample_rate: DEFAULT_SAMPLE_RATE,
            bits_per_sample: DEFAULT_BITS_PER_SAMPLE,
            delay_ms: DEFAULT_DELAY_MS,
            duration_ms: DEFAULT_DURATION_MS,
            initial_seq,
            payload_type: DEFAULT_PAYLOAD_TYPE,
            buffer_ns: DEFAULT_BUFFER_NS,
        };

        let command = format!("VOIPTEST {}\n", params.to_command_args());
        let bytes = command.as_bytes();
        state.write_buffer[..bytes.len()].copy_from_slice(bytes);
        state.voip_params = Some(params);
        // Drop udp_sock here — the OS keeps the port reserved briefly;
        // the UDP thread will re-bind the same port
    }

    let len = state.voip_params.as_ref().map(|p| {
        format!("VOIPTEST {}\n", p.to_command_args()).len()
    }).unwrap_or(0);

    loop {
        let n = state.stream.write(&state.write_buffer[state.write_pos..len])?;
        state.write_pos += n;
        if state.write_pos == len {
            state.write_pos = 0;
            state.read_pos = 0;
            state.phase = TestPhase::VoipReceiveOk;
            state.stream.reregister(poll, state.token, Interest::READABLE)?;
            return Ok(n);
        }
    }
}

pub fn handle_voip_receive_ok(
    poll: &Poll,
    state: &mut MeasurementState,
) -> io::Result<usize> {
    debug!("handle_voip_receive_ok");
    loop {
        let n = state.stream.read(&mut state.read_buffer[state.read_pos..])?;
        if n == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "EOF"));
        }
        state.read_pos += n;

        if state.read_buffer[..state.read_pos].contains(&b'\n') {
            let response = String::from_utf8_lossy(&state.read_buffer[..state.read_pos]);
            // Server may have buffered ACCEPT GETCHUNKS... before OK — find the OK line
            let ok_line = response.lines().find(|l| l.trim().starts_with("OK "));

            if let Some(line) = ok_line {
                let rest = line.trim().strip_prefix("OK ").unwrap_or("");
                let ssrc: u32 = rest.trim().parse().unwrap_or(0);
                state.voip_ssrc = Some(ssrc);
                state.read_pos = 0;

                // Run UDP RTP exchange synchronously in this thread
                // (client threads are already separate per run_threads())
                if let Some(params) = state.voip_params.clone() {
                    let server_ip = state.server_addr.ip();
                    info!("Starting VoIP UDP exchange: {} packets, {}ms delay", params.num_packets(), params.delay_ms);
                    let result_in = run_client_udp(params, ssrc, server_ip);
                    info!(
                        "VoIP incoming result: received={} max_jitter={}ns mean_jitter={}ns",
                        result_in.received_packets, result_in.max_jitter, result_in.mean_jitter
                    );
                    state.voip_result_in = Some(result_in);
                }

                state.phase = TestPhase::VoipSendGetResult;
                state.stream.reregister(poll, state.token, Interest::WRITABLE)?;
                return Ok(n);
            }

            return Err(io::Error::new(io::ErrorKind::InvalidData, "Expected OK <ssrc>"));
        }
    }
}

pub fn handle_voip_send_get_result(
    poll: &Poll,
    state: &mut MeasurementState,
) -> io::Result<usize> {
    debug!("handle_voip_send_get_result");
    let ssrc = state.voip_ssrc.unwrap_or(0);
    let command = format!("GET VOIPRESULT {}\n", ssrc);

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
            state.phase = TestPhase::VoipReceiveResult;
            state.stream.reregister(poll, state.token, Interest::READABLE)?;
            return Ok(n);
        }
    }
}

pub fn handle_voip_receive_result(
    poll: &Poll,
    state: &mut MeasurementState,
) -> io::Result<usize> {
    debug!("handle_voip_receive_result");
    loop {
        let n = state.stream.read(&mut state.read_buffer[state.read_pos..])?;
        if n == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "EOF"));
        }
        state.read_pos += n;

        if state.read_buffer[..state.read_pos].contains(&b'\n') {
            let response = String::from_utf8_lossy(&state.read_buffer[..state.read_pos]);
            // Server may have buffered ACCEPT GETCHUNKS... before VOIPRESULT — find the right line
            let voip_line = response.lines().find(|l| l.trim().starts_with("VOIPRESULT "));

            if let Some(line) = voip_line {
                if let Some(result) = crate::voip::calculator::RtpQoSResult::from_voip_result_string(line.trim()) {
                    info!(
                        "VoIP outgoing result (server measured): received={} max_jitter={}ns",
                        result.received_packets, result.max_jitter
                    );
                    state.voip_result_out = Some(result);
                    state.read_pos = 0;
                    state.phase = TestPhase::VoipCompleted;
                    state.stream.reregister(poll, state.token, Interest::WRITABLE)?;
                    return Ok(n);
                }
            }

            // No valid VOIPRESULT line yet — wait for more data
            if response.lines().count() > 1 || response.contains("VOIPRESULT") {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "Invalid VOIPRESULT"));
            }
        }
    }
}
