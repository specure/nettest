use log::{info, warn};
use mio::{Interest, Poll};
use std::io;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::mioserver::{server::TestState, ServerTestPhase};
use crate::voip::udp::run_server_udp;
use crate::voip::VoipParams;

pub fn handle_voip_send_ok(poll: &Poll, state: &mut TestState) -> io::Result<usize> {
    let ssrc = state.voip_ssrc.unwrap_or(0);
    let response = format!("OK {}\n", ssrc);

    if state.write_pos == 0 {
        let bytes = response.as_bytes();
        state.write_buffer[..bytes.len()].copy_from_slice(bytes);
    }

    let len = response.len();
    loop {
        let n = state.stream.write(&state.write_buffer[state.write_pos..len])?;
        state.write_pos += n;
        if state.write_pos == len {
            state.write_pos = 0;
            state.read_pos = 0;
            state.measurement_state = ServerTestPhase::AcceptCommandSend;
            state.stream.reregister(poll, state.token, Interest::WRITABLE)?;
            return Ok(n);
        }
    }
}

pub fn handle_voip_send_result(poll: &Poll, state: &mut TestState) -> io::Result<usize> {
    let result_arc = match &state.voip_result {
        Some(a) => a.clone(),
        None => {
            warn!("VoIP result store is missing");
            let response = "VOIPRESULT 0 0 0 0 0 0 0 0 0 0\n";
            state.write_buffer[..response.len()].copy_from_slice(response.as_bytes());
            let n = state.stream.write(&response.as_bytes()[state.write_pos..response.len()])?;
            state.write_pos = 0;
            state.measurement_state = ServerTestPhase::AcceptCommandSend;
            state.stream.reregister(poll, state.token, Interest::WRITABLE)?;
            return Ok(n);
        }
    };

    // Wait for UDP thread to finish (should be quick at this point)
    let result = loop {
        let guard = result_arc.lock().unwrap();
        if let Some(r) = guard.clone() {
            break r;
        }
        drop(guard);
        thread::sleep(Duration::from_millis(10));
    };

    let response = format!("{}\n", result.to_voip_result_string());

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
            info!(
                "VoIP result sent: received={} max_jitter={}ns",
                result.received_packets, result.max_jitter
            );
            state.write_pos = 0;
            state.read_pos = 0;
            state.voip_result = None;
            state.voip_params = None;
            state.voip_ssrc = None;
            state.measurement_state = ServerTestPhase::AcceptCommandSend;
            state.stream.reregister(poll, state.token, Interest::WRITABLE)?;
            return Ok(n);
        }
    }
}

pub fn start_voip_udp_thread(
    params: VoipParams,
    ssrc: u32,
    client_ip: std::net::IpAddr,
    udp_server: std::sync::Arc<crate::udp::SharedUdpServer>,
) -> Arc<Mutex<Option<crate::voip::RtpQoSResult>>> {
    let result_store = Arc::new(Mutex::new(None));
    let store_clone = result_store.clone();

    thread::spawn(move || {
        run_server_udp(params, ssrc, client_ip, store_clone, udp_server);
    });

    result_store
}
