use crate::config::constants::{MAX_CHUNK_SIZE, MIN_CHUNK_SIZE, RESP_ERR};
use rand::RngCore;
use std::error::Error;
use std::time::Instant;
use log::{debug, error, trace};
use crate::stream::Stream;

pub async fn handle_get_time(stream: &mut Stream, command: &str) -> Result<(), Box<dyn Error + Send + Sync>> {
    debug!("handle_get_time: starting");

    // Parse command parts after GETTIME
    let parts: Vec<&str> = command[7..].trim().split_whitespace().collect();

    // Validate command format exactly like in C code
    if parts.len() != 1 && parts.len() != 2 {
        stream.write_all(RESP_ERR.as_bytes()).await?;
        return Ok(());
    }

    // Parse duration using strtoul-like parsing
    let duration = match parts[0].parse::<u64>() {
        Ok(d) => d,
        Err(_) => {
            stream.write_all(RESP_ERR.as_bytes()).await?;
            return Ok(());
        }
    };

    // Parse and validate chunk size
    let chunk_size = if parts.len() == 1 {
        MIN_CHUNK_SIZE
    } else {
        match parts[1].parse::<usize>() {
            Ok(size) => {
                if size < MIN_CHUNK_SIZE || size > MAX_CHUNK_SIZE {
                    stream.write_all(RESP_ERR.as_bytes()).await?;
                    return Ok(());
                }
                size
            }
            Err(_) => {
                stream.write_all(RESP_ERR.as_bytes()).await?;
                return Ok(());
            }
        }
    };

    // Проверяем длительность
    if duration < 2 {
        error!("Duration must be at least 2 seconds");
        stream.write_all(RESP_ERR.as_bytes()).await?;
        return Ok(());
    }

    let mut total_bytes = 0;
    let start_time = Instant::now();
    let mut buffer = vec![0u8; chunk_size];

    debug!("Starting GETTIME: duration={}s, chunk_size={}", duration, chunk_size);

    // Отправляем данные до истечения времени
    while start_time.elapsed().as_secs() < duration {
        // Создаем новый генератор для каждого чанка
        rand::thread_rng().fill_bytes(&mut buffer[..chunk_size - 1]);
        
        // Устанавливаем последний байт в 0 для всех чанков, кроме последнего
        buffer[chunk_size - 1] = 0x00;
        
        // Отправляем чанк
        stream.write_all(&buffer).await?;
        stream.flush().await?;
        total_bytes += chunk_size;
        
        trace!("Sent chunk: first 32 bytes: {:?}", &buffer[..std::cmp::min(32, chunk_size)]);
        trace!("Sent chunk: last 32 bytes: {:?}", &buffer[chunk_size.saturating_sub(32)..]);
        trace!("Last byte: 0x{:02X}", buffer[chunk_size - 1]);
    }

    // Отправляем последний чанк с терминатором
    rand::thread_rng().fill_bytes(&mut buffer[..chunk_size - 1]);
    buffer[chunk_size - 1] = 0xFF; // Устанавливаем терминатор
    stream.write_all(&buffer).await?;
    total_bytes += chunk_size;
    
    trace!("Sent final chunk: first 32 bytes: {:?}", &buffer[..std::cmp::min(32, chunk_size)]);
    trace!("Sent final chunk: last 32 bytes: {:?}", &buffer[chunk_size.saturating_sub(32)..]);
    trace!("Last byte: 0x{:02X}", buffer[chunk_size - 1]);

    debug!("All data sent. Total bytes: {}", total_bytes);

    // Ждем ответ OK от клиента
    debug!("Waiting for OK response from client");
    let mut response = [0u8; 1024];
    let n = stream.read(&mut response).await?;
    let response_str = String::from_utf8_lossy(&response[..n]);
    debug!("Received response from client: '{}'", response_str.trim());
    debug!("Response bytes: {:?}", &response[..n]);
        
    if response_str.trim() != "OK" {
        error!("Expected OK response, got: {}", response_str);
        return Err("Invalid response from client".into());
    }

    // Отправляем TIME ответ
    debug!("Sending TIME response");
    let time_ns = start_time.elapsed().as_nanos();
    let time_response = format!("TIME {}\n", time_ns);
    debug!("Sending TIME response: '{}'", time_response.trim());
    debug!("TIME response bytes: {:?}", time_response.as_bytes());
    stream.write_all(time_response.as_bytes()).await?;
    stream.flush().await?;
    debug!("TIME response sent and flushed");

    Ok(())
}

