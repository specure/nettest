use std::{io, time::Instant};

use hmac::{Hmac, Mac};
use log::debug;
use mio::{Interest, Poll};
use sha1::Sha1;

use crate::mioserver::{server::TestState, ServerTestPhase};

pub fn handle_signed_result(poll: &Poll, state: &mut TestState) -> Result<usize, std::io::Error> {
    debug!("handle_signed_result");

    if state.write_pos == 0 {
        let message = format!(
            "GETTIME:({} {}); PUTTIMERESULT:({} {}); CLIENT_IP:{}; TIMESTAMP:{}\n",
            state.total_bytes_received,
            state.received_time_ns.unwrap(),
            state.total_bytes_sent,
            state.sent_time_ns.unwrap(),
            state.client_addr.unwrap(),
            Instant::now().elapsed().as_nanos()
        );

        let secret_key = state.sig_key.as_ref().unwrap();
        let signature = sign_message(&message, &secret_key)?;

        debug!("Signed message: {}", message);
        debug!("Signature: {}", signature);

        let envelope = format!("{}:{}", message, signature);

        state.write_pos = envelope.len();
        state.write_buffer[0..state.write_pos].copy_from_slice(envelope.as_bytes());
    }

    loop {
        let n = state
            .stream
            .write(&mut state.chunk_buffer[state.read_pos..])?;
        if n == 0 {
            debug!("EOF");
            return Err(io::Error::new(io::ErrorKind::Other, "EOF"));
        }
        state.read_pos += n;
        if state.read_pos >= state.write_pos {
            state.write_pos = 0;
            state.read_pos = 0;
            state.measurement_state = ServerTestPhase::SignedResultReceiveOk;
            state.stream.reregister(poll, state.token, Interest::READABLE)?;
            return Ok(n);
        }
    }
}

pub fn handle_signed_result_receive_ok(poll: &Poll, state: &mut TestState) -> Result<usize, std::io::Error> {
    let ok = b"OK\n";
    loop {
        let n = state.stream.read(&mut state.read_buffer)?;
        if n == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "EOF"));
        }
        state.read_pos += n;
        if state.read_buffer[0..ok.len()] == ok[..] {
            let time = state.clock.unwrap().elapsed().as_nanos();
            state.clock = None;
            state.sent_time_ns = Some(time);
            state.measurement_state = ServerTestPhase::AcceptCommandSend;
            state.read_pos = 0;
            state.stream.reregister(poll, state.token, Interest::WRITABLE)?;
            return Ok(n);
        }
    }
}

fn sign_message(message: &str, secret_key: &str) -> Result<String, std::io::Error> {
    type HmacSha256 = Hmac<Sha1>;

    let mut mac = HmacSha256::new_from_slice(secret_key.as_bytes())
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    mac.update(message.as_bytes());
    let result = mac.finalize();
    let signature = base64::encode(result.into_bytes());

    Ok(signature)
}
