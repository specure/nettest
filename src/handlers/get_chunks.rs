use std::time::Instant;
use log::{debug, error, info};
use crate::config::constants::{CHUNK_SIZE, MIN_CHUNK_SIZE, MAX_CHUNK_SIZE, MAX_CHUNKS, RESP_ERR};
use fastrand::Rng;
use crate::stream::Stream;
use crate::utils::random_buffer::get_random_slice;

pub async fn handle_get_chunks(
    stream: &mut Stream,
    command: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let parts: Vec<&str> = command.split_whitespace().collect();
    if parts.len() < 2 || parts.len() > 3 {
        stream.write_all(RESP_ERR.as_bytes()).await?;
        return Err("Invalid number of arguments for GETCHUNKS".into());
    }

    let chunks = match parts[1].parse::<u32>() {
        Ok(n) if n > 0 && n <= MAX_CHUNKS as u32 => n,
        _ => {
            stream.write_all(RESP_ERR.as_bytes()).await?;
            return Err("Invalid chunks value or exceeds MAX_CHUNKS".into());
        }
    };

    let chunk_size = if parts.len() == 3 {
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
    
    debug!("Starting GETCHUNKS process: chunks={}, chunk_size={}", chunks, chunk_size);

    let mut total_bytes = 0;
    let start_time = Instant::now();
    let mut buffer = vec![0u8; chunk_size];
    let mut chunks_sent = 0;
    let mut rng = Rng::new();

    while chunks_sent < chunks {
        // Fill buffer with random data
        let offset = 0;
        info!("Sending chunk {} of {}", chunks_sent + 1, chunks);
        let random_bytes = get_random_slice(offset..offset + chunk_size - 1);
        buffer[..chunk_size - 1].copy_from_slice(&random_bytes);
        info!("Chunk {} sent", chunks_sent + 1);
        chunks_sent += 1;
        if chunks_sent >= chunks {
            buffer[chunk_size - 1] = 0xFF;
        } else {
            buffer[chunk_size - 1] = 0x00;
        }

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
    debug!(
        "GETCHUNKS completed: sent {} chunks ({} bytes) in {:?}",
        chunks_sent,
        total_bytes,
        elapsed
    );

    let mut response = vec![0u8; 1024];
    let n = stream.read(&mut response).await?;
    let response_str = String::from_utf8_lossy(&response[..n]);
    debug!("Client response: {}", response_str);

    if !response_str.trim().eq("OK") {
        error!("Expected OK from client, got: {}", response_str);
        stream.write_all(RESP_ERR.as_bytes()).await?;
        return Err("Invalid client response".into());
    }

    let time_ns = elapsed.as_nanos();
    let time_response = format!("TIME {}\n", time_ns);
    stream.write_all(time_response.as_bytes()).await?;

    Ok(())
}
