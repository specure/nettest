use std::{io, time::Instant};

use hmac::{Hmac, Mac};
use log::debug;
use mio::{Interest, Poll};
use sha2::Sha256;
use base64;

use crate::mioserver::{server::TestState, ServerTestPhase};

pub fn handle_signed_result(poll: &Poll, state: &mut TestState) -> Result<usize, std::io::Error> {
    println!("handle_signed_result");

    let message = format!(
        "GETTIME:({} {}); PUTTIMERESULT:({} {}); CLIENT_IP:{}; TIMESTAMP:{};",
        state.total_bytes_received,
        state.received_time_ns.unwrap(),
        state.total_bytes_sent,
        state.sent_time_ns.unwrap(),
        state.client_addr.unwrap(),
        Instant::now().elapsed().as_nanos()
    );

    if state.sig_key.is_none() {
        let secret_key = generate_secret_key();
        state.sig_key = Some(secret_key.clone());
    }

    let secret_key = state.sig_key.as_ref().unwrap();
    let signature = sign_message(&message, &secret_key)?;

    debug!("Signed message: {} Signature: {} Secret key: {}", message, signature, secret_key);

    let envelope = format!("{}:{}\n", message, signature);
    if state.write_pos == 0 {
        debug!("envelope: {}", envelope);
        state.write_buffer[0..envelope.len()].copy_from_slice(envelope.as_bytes());
    }

    loop {
        let n = state
            .stream
            .write(&mut state.write_buffer[state.write_pos..envelope.len()])?;
        state.write_pos += n;
        if state.write_pos >= envelope.len() {
            state.write_pos = 0;
            state.read_pos = 0;
            state.measurement_state = ServerTestPhase::SignedResultReceiveOk;
            state.stream.reregister(poll, state.token, Interest::READABLE)?;
            return Ok(n);
        }
    }
}

pub fn handle_signed_result_receive_ok(poll: &Poll, state: &mut TestState) -> Result<usize, std::io::Error> {
    println!("handle_signed_result_receive_ok");
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
    type HmacSha256 = Hmac<Sha256>;

    let decoded_key = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, secret_key)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, format!("Failed to decode base64 key: {}", e)))?;

    let mut mac = HmacSha256::new_from_slice(&decoded_key)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    mac.update(message.as_bytes());
    let result = mac.finalize();
    let signature = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, result.into_bytes());

    Ok(signature)
}

//TODO: or get from file
pub fn generate_secret_key() -> String {
    let mut secret_key = [0u8; 32];
    getrandom::fill(&mut secret_key).expect("failed to read from the OS CSPRNG");
    base64::Engine::encode(&base64::engine::general_purpose::STANDARD, secret_key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn secret_key_is_32_bytes_of_valid_base64() {
        let key = generate_secret_key();
        let decoded =
            base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &key)
                .expect("secret key must be valid base64");
        assert_eq!(decoded.len(), 32, "HMAC key must be 32 bytes");
    }

    #[test]
    fn secret_keys_are_unique() {
        // A repeated key would let one client forge another's signed result.
        let keys: HashSet<String> = (0..1000).map(|_| generate_secret_key()).collect();
        assert_eq!(keys.len(), 1000, "generate_secret_key produced a duplicate");
    }

    #[test]
    fn secret_key_is_usable_for_signing() {
        let key = generate_secret_key();
        let signature = sign_message("GETTIME:(1 2); CLIENT_IP:127.0.0.1;", &key)
            .expect("signing with a generated key must succeed");
        assert!(!signature.is_empty());
        // HMAC-SHA256 is 32 bytes, base64-encoded as 44 chars.
        assert_eq!(signature.len(), 44);
    }
}
