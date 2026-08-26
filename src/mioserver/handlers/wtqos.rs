//! Control-channel side of the browser QoS test.
//!
//! These handlers only talk to the [registry](crate::wtqos::registry): they hand
//! commands to the QUIC session task and read back its counters. Nothing here
//! may block — a mio worker thread is shared by many connections.
//!
//! Command shapes mirror the native `UDPTEST` / `GET UDPRESULT` pair, minus the
//! ports (the QUIC session is already bidirectional):
//!
//! ```text
//! GET WTURL                        -> WTURL <port> <path> <cert-sha256-base64|-> <selfsigned|trusted>
//! WTTEST OUT <n> <delay_ms> <uuid> -> OK
//! GET WTRESULT OUT                 -> RCV <received> <duplicates> <out_of_order> <jitter_ns> <max_delta_ns>
//! WTTEST IN  <n> <delay_ms> <uuid> -> OK
//! GET WTRESULT IN                  -> SNT <sent>
//! ```
//!
//! The hash is always published (it is public handshake data); the trailing
//! word says whether a client *must* use it: `selfsigned` means pass it in
//! `serverCertificateHashes`, `trusted` means connect normally. A `WTURL 0 - -
//! -` reply means the endpoint is off.

use log::{debug, warn};
use mio::{Interest, Poll};
use std::io;

use crate::mioserver::{server::TestState, ServerTestPhase};
use crate::wtqos::registry::{registry, SessionCommand};

/// Write `response` and return the connection to command-reading state.
fn respond(poll: &Poll, state: &mut TestState, response: &str) -> io::Result<usize> {
    if state.write_pos == 0 {
        state.write_buffer[..response.len()].copy_from_slice(response.as_bytes());
    }
    let len = response.len();
    loop {
        let n = state.stream.write(&state.write_buffer[state.write_pos..len])?;
        state.write_pos += n;
        if state.write_pos == len {
            state.write_pos = 0;
            state.read_pos = 0;
            state.measurement_state = ServerTestPhase::AcceptCommandReceive;
            state.stream.reregister(poll, state.token, Interest::READABLE)?;
            return Ok(n);
        }
    }
}

/// `GET WTURL` — where to open the QUIC session, and how to trust it.
pub fn handle_wt_send_url(poll: &Poll, state: &mut TestState) -> io::Result<usize> {
    let response = match registry() {
        Some(registry) => format!(
            "WTURL {} {} {} {}\n",
            registry.port,
            crate::wtqos::WT_PATH,
            registry.cert_hash_base64.as_deref().unwrap_or("-"),
            if registry.self_signed { "selfsigned" } else { "trusted" }
        ),
        None => "WTURL 0 - - -\n".to_string(),
    };
    debug!("WTURL -> {}", response.trim());
    respond(poll, state, &response)
}

/// `WTTEST OUT/IN` — acknowledge; the session task does the work.
pub fn handle_wt_send_ok(poll: &Poll, state: &mut TestState) -> io::Result<usize> {
    respond(poll, state, "OK\n")
}

/// `GET WTRESULT OUT` — how much of the client's train actually arrived.
pub fn handle_wt_send_result_out(poll: &Poll, state: &mut TestState) -> io::Result<usize> {
    let stats = state
        .wt_uuid
        .and_then(|uuid| registry()?.get(&uuid))
        .and_then(|session| session.out.lock().ok().map(|s| *s));
    let response = match stats {
        Some(s) => format!(
            "RCV {} {} {} {} {}\n",
            s.counted, s.duplicates, s.out_of_order, s.jitter_ns, s.max_delta_ns
        ),
        None => {
            warn!("GET WTRESULT OUT without a registered session");
            "RCV 0 0 0 0 0\n".to_string()
        }
    };
    respond(poll, state, &response)
}

/// `GET WTRESULT IN` — how many datagrams the server put on the wire, which is
/// what the client compares its own count against.
pub fn handle_wt_send_result_in(poll: &Poll, state: &mut TestState) -> io::Result<usize> {
    let stats = state
        .wt_uuid
        .and_then(|uuid| registry()?.get(&uuid))
        .and_then(|session| session.inbound.lock().ok().map(|s| *s));
    let response = match stats {
        Some(s) => format!("SNT {}\n", s.counted),
        None => {
            warn!("GET WTRESULT IN without a registered session");
            "SNT 0\n".to_string()
        }
    };
    respond(poll, state, &response)
}

/// Parse and act on a `WTTEST OUT|IN <n> <delay_ms> <uuid_hex>` command.
/// Returns false when the command is malformed or the session is unknown.
pub fn arm_test(state: &mut TestState, command: &str) -> bool {
    let parts: Vec<&str> = command.trim().split_whitespace().collect();
    // WTTEST <dir> <n> <delay_ms> <uuid>
    if parts.len() < 5 {
        return false;
    }
    let direction = parts[1];
    let count: u32 = parts[2].parse().unwrap_or(10);
    let delay_ms: u64 = parts[3].parse().unwrap_or(200);
    let uuid = match crate::wtqos::registry::uuid_from_hex(parts[4]) {
        Some(uuid) => uuid,
        None => return false,
    };
    state.wt_uuid = Some(uuid);

    let session = match registry().and_then(|r| r.get(&uuid)) {
        Some(session) => session,
        None => {
            warn!("WTTEST for an unregistered QUIC session");
            return false;
        }
    };
    let command = match direction {
        "OUT" => SessionCommand::ArmOut { expected: count },
        "IN" => SessionCommand::SendTrain { count, delay_ms },
        _ => return false,
    };
    session.send(command)
}
