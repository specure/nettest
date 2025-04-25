use std::time::Instant;
use log::{info, debug, error};
use crate::config::constants::{CHUNK_SIZE, MIN_CHUNK_SIZE, MAX_CHUNK_SIZE, MAX_CHUNKS, RESP_ERR};
use rand::RngCore;
use crate::stream::Stream;

pub async fn handle_get_chunks(
    stream: &mut Stream,
    command: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Парсим команду: GETCHUNKS <chunks> [chunk_size]
    let parts: Vec<&str> = command.split_whitespace().collect();
    if parts.len() < 2 || parts.len() > 3 {
        stream.write_all(RESP_ERR.as_bytes()).await?;
        return Err("Invalid number of arguments for GETCHUNKS".into());
    }

    // Парсим количество чанков
    let chunks = match parts[1].parse::<u32>() {
        Ok(n) if n > 0 && n <= MAX_CHUNKS as u32 => n,
        _ => {
            stream.write_all(RESP_ERR.as_bytes()).await?;
            return Err("Invalid chunks value or exceeds MAX_CHUNKS".into());
        }
    };

    // Определяем размер чанка
    let chunk_size = if parts.len() == 3 {
        // Если указан размер чанка, проверяем его
        match parts[2].parse::<usize>() {
            Ok(size) if size >= MIN_CHUNK_SIZE && size <= MAX_CHUNK_SIZE => size,
            _ => {
                stream.write_all(RESP_ERR.as_bytes()).await?;
                return Err("Invalid chunk size".into());
            }
        }
    } else {
        CHUNK_SIZE
    };
    
    info!("Starting GETCHUNKS process: chunks={}, chunk_size={}", chunks, chunk_size);

    let mut total_bytes = 0;
    let start_time = Instant::now();
    let mut buffer = vec![0u8; chunk_size];
    let mut chunks_sent = 0;

    // Отправляем указанное количество чанков
    while chunks_sent < chunks {
        // Генерируем случайные данные для чанка
        rand::thread_rng().fill_bytes(&mut buffer[..chunk_size - 1]);
        
        // Устанавливаем последний байт
        chunks_sent += 1;
        if chunks_sent >= chunks {
            buffer[chunk_size - 1] = 0xFF; // Последний чанк
        } else {
            buffer[chunk_size - 1] = 0x00; // Обычный чанк
        }

        // Отправляем чанк
        match stream.write_all(&buffer).await {
            Ok(_) => {
                stream.flush().await?;
                total_bytes += chunk_size;
                debug!("Sent chunk {}/{}: last_byte=0x{:02X}", chunks_sent, chunks, buffer[chunk_size - 1]);
            },
            Err(e) => {
                error!("Failed to send chunk: {}", e);
                stream.write_all(RESP_ERR.as_bytes()).await?;
                return Err(e.into());
            }
        }
    }

    let elapsed = start_time.elapsed();
    info!(
        "GETCHUNKS completed: sent {} chunks ({} bytes) in {:?}",
        chunks_sent,
        total_bytes,
        elapsed
    );

    // Ждем OK от клиента
    let mut response = vec![0u8; 1024];
    let n = stream.read(&mut response).await?;
    let response_str = String::from_utf8_lossy(&response[..n]);
    debug!("Client response: {}", response_str);

    if !response_str.trim().eq("OK") {
        error!("Expected OK from client, got: {}", response_str);
        stream.write_all(RESP_ERR.as_bytes()).await?;
        return Err("Invalid client response".into());
    }

    // Отправляем время выполнения
    let time_ns = elapsed.as_nanos();
    let time_response = format!("TIME {}\n", time_ns);
    stream.write_all(time_response.as_bytes()).await?;

    Ok(())
}
