use crate::config::constants::{MAX_CHUNK_SIZE, MIN_CHUNK_SIZE, RESP_ERR};
use std::error::Error;
use std::time::Instant;
use log::{debug, error, trace};
use crate::stream::Stream;
use fastrand::Rng;
use crate::utils::random_buffer::get_random_slice;

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

    // Check duration
    if duration < 2 {
        error!("Duration must be at least 2 seconds");
        stream.write_all(RESP_ERR.as_bytes()).await?;
        return Ok(());
    }

    let mut total_bytes = 0;
    let start_time = Instant::now();
    let mut buffer = vec![0u8; chunk_size];

    let mut random_offset = 0;
    // Send data until time expires
    while start_time.elapsed().as_secs() < duration {
        let random_data = get_random_slice(random_offset..random_offset + chunk_size - 1);
        buffer[..chunk_size - 1].copy_from_slice(&random_data);
        random_offset = (random_offset + chunk_size - 1) % crate::utils::random_buffer::RANDOM_BUFFER.len();

        // Set last byte to 0 for all chunks except the last one
        buffer[chunk_size - 1] = 0x00;

        // Send chunk
        stream.write_all(&buffer).await?;
        stream.flush().await?;
        total_bytes += chunk_size;

    }

    // Send final chunk with terminator
    let random_data = get_random_slice(random_offset..random_offset + chunk_size - 1);
    buffer[..chunk_size - 1].copy_from_slice(&random_data);
    buffer[chunk_size - 1] = 0xFF; // Set terminator
    stream.write_all(&buffer).await?;
    total_bytes += chunk_size;

    debug!("All data sent. Total bytes: {}", total_bytes);

    // Wait for OK response from client
    let mut response = [0u8; 1024];
    let n = stream.read(&mut response).await?;
    let response_str = String::from_utf8_lossy(&response[..n]);

    if response_str.trim() != "OK" {
        error!("Expected OK response, got: {}", response_str);
        return Err("Invalid response from client".into());
    }

    // Send TIME response
    let time_ns = start_time.elapsed().as_nanos();
    let time_response = format!("TIME {}\n", time_ns);
    stream.write_all(time_response.as_bytes()).await?;
    stream.flush().await?;
    debug!("TIME response sent and flushed");

    Ok(())
}

