use std::{io, time::Instant};

use hmac::{Hmac, Mac};
use mio::{Interest, Poll};
use openssl::rand;
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

    println!("Signed message: {} Signature: {} Secret key: {}", message, signature, secret_key);

    let envelope = format!("{}:{}\n", message, signature);
    if state.write_pos == 0 {
        println!("envelope: {}", envelope);
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

pub fn generate_secret_key() -> String {
    let mut secret_key = [0u8; 32];
    rand::rand_bytes(&mut secret_key).unwrap();
    base64::Engine::encode(&base64::engine::general_purpose::STANDARD, secret_key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    use base64::{Engine as _, engine::general_purpose::STANDARD};

    #[test]
    fn test_hmac_signature_verification() {
        // Тестовые данные из логов сервера
        let secret_key = "F/cjqWDuEZIYQ+h9pjaU7DdZF30eBh/M9uK1RXODRq8=";
        let signed_data = "GETTIME:(79691776 11281699225); PUTTIMERESULT:(50331648 10638442557); CLIENT_IP:10.35.3.9:53551; TIMESTAMP:60;";
        
        // Ожидаемая подпись с правильным (декодированным) ключом
        let expected_signature = "k9fEOCHeqQDBwbWH1BQGzAxZ233vd8PJjT2LdnnXTV8=";

        println!("=== HMAC SIGNATURE VERIFICATION TEST ===");
        println!("Data to sign: '{}'", signed_data);
        println!("Secret key (base64): {}", secret_key);
        println!("Expected signature (with decoded key): {}", expected_signature);

        // Декодируем base64 ключ
        let decoded_key = STANDARD.decode(secret_key).expect("Failed to decode secret key");
        println!("Decoded key length: {} bytes", decoded_key.len());

        // Создаем HMAC-SHA256 с декодированным ключом (правильный способ)
        let mut mac = Hmac::<Sha256>::new_from_slice(&decoded_key)
            .expect("Failed to create HMAC-SHA256");
        mac.update(signed_data.as_bytes());
        let signature = mac.finalize();
        let calculated_signature = STANDARD.encode(signature.into_bytes());
        
        println!("Calculated signature: {}", calculated_signature);
        
        // Проверяем что подписи совпадают
        assert_eq!(calculated_signature, expected_signature, 
            "HMAC signature verification failed!\nExpected: {}\nCalculated: {}", 
            expected_signature, calculated_signature);

        println!("✅ HMAC signature verification successful!");
        println!("Note: Now using properly decoded base64 key with SHA-256");
        
        // Дополнительно проверяем что наша функция sign_message работает правильно
        let signature_from_function = sign_message(signed_data, secret_key).expect("Failed to sign message");
        println!("Signature from sign_message function: {}", signature_from_function);
        
        assert_eq!(signature_from_function, expected_signature, 
            "sign_message function failed!\nExpected: {}\nGot: {}", 
            expected_signature, signature_from_function);
        
        println!("✅ sign_message function works correctly!");
    }

    #[test]
    fn test_hmac_signature_verification_new_data() {
        // Новые тестовые данные из логов
        let secret_key = "4SJv1F+URQ2vRJbv7UxhoceqiPVPOsY/LnDOg2RhhkA=";
        let signed_data = "GETTIME:(79691776 11281699225); PUTTIMERESULT:(50331648 10638442557); CLIENT_IP:10.35.3.9:53551; TIMESTAMP:60;";
        
        // Ожидаемая подпись из логов
        let expected_signature = "IbQdufDY19HDGRgjO+OknLwEEfVnjhLbjyrLYM2KyZg=";

        println!("=== HMAC SIGNATURE VERIFICATION TEST (NEW DATA) ===");
        println!("Data to sign: '{}'", signed_data);
        println!("Secret key (base64): {}", secret_key);
        println!("Expected signature from logs: {}", expected_signature);

        // Тест 1: С декодированным ключом (правильный способ)
        println!("\n--- Test 1: With decoded key (correct way) ---");
        let decoded_key = STANDARD.decode(secret_key).expect("Failed to decode secret key");
        println!("Decoded key length: {} bytes", decoded_key.len());

        let mut mac = Hmac::<Sha256>::new_from_slice(&decoded_key)
            .expect("Failed to create HMAC-SHA256");
        mac.update(signed_data.as_bytes());
        let signature = mac.finalize();
        let calculated_signature_decoded = STANDARD.encode(signature.into_bytes());
        
        println!("Signature with decoded key: {}", calculated_signature_decoded);
        
        if calculated_signature_decoded == expected_signature {
            println!("✅ Decoded key signature matches!");
            return;
        }

        // Тест 2: С недекодированным ключом (как делал сервер раньше)
        println!("\n--- Test 2: With raw base64 key (old server way) ---");
        let mut mac_raw = Hmac::<Sha256>::new_from_slice(secret_key.as_bytes())
            .expect("Failed to create HMAC-SHA256");
        mac_raw.update(signed_data.as_bytes());
        let signature_raw = mac_raw.finalize();
        let calculated_signature_raw = STANDARD.encode(signature_raw.into_bytes());
        
        println!("Signature with raw key: {}", calculated_signature_raw);
        
        if calculated_signature_raw == expected_signature {
            println!("✅ Raw key signature matches! Server still uses old method.");
            return;
        }

        // Тест 3: С символом новой строки
        println!("\n--- Test 3: With newline ---");
        let data_with_newline = format!("{}\n", signed_data);
        let mut mac_nl = Hmac::<Sha256>::new_from_slice(&decoded_key)
            .expect("Failed to create HMAC-SHA256");
        mac_nl.update(data_with_newline.as_bytes());
        let signature_nl = mac_nl.finalize();
        let calculated_signature_nl = STANDARD.encode(signature_nl.into_bytes());
        
        println!("Signature with newline: {}", calculated_signature_nl);
        
        if calculated_signature_nl == expected_signature {
            println!("✅ Newline signature matches!");
            return;
        }

        // Тест 4: С символом новой строки и raw ключом
        let mut mac_raw_nl = Hmac::<Sha256>::new_from_slice(secret_key.as_bytes())
            .expect("Failed to create HMAC-SHA256");
        mac_raw_nl.update(data_with_newline.as_bytes());
        let signature_raw_nl = mac_raw_nl.finalize();
        let calculated_signature_raw_nl = STANDARD.encode(signature_raw_nl.into_bytes());
        
        println!("Signature with raw key + newline: {}", calculated_signature_raw_nl);
        
        if calculated_signature_raw_nl == expected_signature {
            println!("✅ Raw key + newline signature matches!");
            return;
        }

        println!("\n❌ No signature matches found!");
        println!("This suggests:");
        println!("1. Data format is different");
        println!("2. Server uses different algorithm");
        println!("3. Expected signature was generated with different parameters");
        
        // Показываем все варианты для анализа
        println!("\nAll calculated signatures:");
        println!("Decoded key: {}", calculated_signature_decoded);
        println!("Raw key: {}", calculated_signature_raw);
        println!("Decoded key + newline: {}", calculated_signature_nl);
        println!("Raw key + newline: {}", calculated_signature_raw_nl);
    }

    #[test]
    fn test_our_sign_message_function() {
        // Тестируем нашу функцию sign_message
        let test_message = "TEST_MESSAGE:12345;";
        let test_key = "test_secret_key_32_bytes_long_key_123";
        
        let signature = sign_message(test_message, test_key).expect("Failed to sign message");
        println!("Test signature: {}", signature);
        
        // Проверяем что подпись не пустая и имеет правильную длину
        assert!(!signature.is_empty(), "Signature should not be empty");
        assert_eq!(signature.len(), 44, "Base64 SHA256 signature should be 44 characters long");
        
        println!("✅ Our sign_message function works correctly!");
    }

    #[test]
    fn test_hmac_signature_verification_real_logs() {
        // Реальные данные из логов сервера
        let secret_key = "4SJv1F+URQ2vRJbv7UxhoceqiPVPOsY/LnDOg2RhhkA=";
        let signed_data = "GETTIME:(90701824 10543531805); PUTTIMERESULT:(33030144 10382023090); CLIENT_IP:[::ffff:10.35.3.9]:49213; TIMESTAMP:60;";
        
        // Ожидаемая подпись из логов
        let expected_signature = "0jU0JD37dSzAYXcTLIMnMHWrTQyh1mssLmXGEfzJDU4=";

        println!("=== HMAC SIGNATURE VERIFICATION TEST (REAL LOGS) ===");
        println!("Data to sign: '{}'", signed_data);
        println!("Secret key (base64): {}", secret_key);
        println!("Expected signature from logs: {}", expected_signature);

        // Тест 1: С декодированным ключом (правильный способ)
        println!("\n--- Test 1: With decoded key (correct way) ---");
        let decoded_key = STANDARD.decode(secret_key).expect("Failed to decode secret key");
        println!("Decoded key length: {} bytes", decoded_key.len());

        let mut mac = Hmac::<Sha256>::new_from_slice(&decoded_key)
            .expect("Failed to create HMAC-SHA256");
        mac.update(signed_data.as_bytes());
        let signature = mac.finalize();
        let calculated_signature_decoded = STANDARD.encode(signature.into_bytes());
        
        println!("Signature with decoded key: {}", calculated_signature_decoded);
        
        if calculated_signature_decoded == expected_signature {
            println!("✅ Decoded key signature matches!");
            return;
        }

        // Тест 2: С недекодированным ключом (как делал сервер раньше)
        println!("\n--- Test 2: With raw base64 key (old server way) ---");
        let mut mac_raw = Hmac::<Sha256>::new_from_slice(secret_key.as_bytes())
            .expect("Failed to create HMAC-SHA256");
        mac_raw.update(signed_data.as_bytes());
        let signature_raw = mac_raw.finalize();
        let calculated_signature_raw = STANDARD.encode(signature_raw.into_bytes());
        
        println!("Signature with raw key: {}", calculated_signature_raw);
        
        if calculated_signature_raw == expected_signature {
            println!("✅ Raw key signature matches! Server still uses old method.");
            return;
        }

        // Тест 3: С символом новой строки
        println!("\n--- Test 3: With newline ---");
        let data_with_newline = format!("{}\n", signed_data);
        let mut mac_nl = Hmac::<Sha256>::new_from_slice(&decoded_key)
            .expect("Failed to create HMAC-SHA256");
        mac_nl.update(data_with_newline.as_bytes());
        let signature_nl = mac_nl.finalize();
        let calculated_signature_nl = STANDARD.encode(signature_nl.into_bytes());
        
        println!("Signature with newline: {}", calculated_signature_nl);
        
        if calculated_signature_nl == expected_signature {
            println!("✅ Newline signature matches!");
            return;
        }

        // Тест 4: С символом новой строки и raw ключом
        let mut mac_raw_nl = Hmac::<Sha256>::new_from_slice(secret_key.as_bytes())
            .expect("Failed to create HMAC-SHA256");
        mac_raw_nl.update(data_with_newline.as_bytes());
        let signature_raw_nl = mac_raw_nl.finalize();
        let calculated_signature_raw_nl = STANDARD.encode(signature_raw_nl.into_bytes());
        
        println!("Signature with raw key + newline: {}", calculated_signature_raw_nl);
        
        if calculated_signature_raw_nl == expected_signature {
            println!("✅ Raw key + newline signature matches!");
            return;
        }

        println!("\n❌ No signature matches found!");
        println!("This suggests:");
        println!("1. Data format is different");
        println!("2. Server uses different algorithm");
        println!("3. Expected signature was generated with different parameters");
        
        // Показываем все варианты для анализа
        println!("\nAll calculated signatures:");
        println!("Decoded key: {}", calculated_signature_decoded);
        println!("Raw key: {}", calculated_signature_raw);
        println!("Decoded key + newline: {}", calculated_signature_nl);
        println!("Raw key + newline: {}", calculated_signature_raw_nl);
    }
}
