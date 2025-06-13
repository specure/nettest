use crate::config::constants::{MAX_CHUNK_SIZE, MIN_CHUNK_SIZE, RESP_ERR};
use bytes::Bytes;
use std::error::Error;
use std::time::Instant;

use crate::stream::Stream;
use crate::utils::random_buffer::get_random_slice;
use log::{debug, error};

fn generate_chunks(num_chunks: usize, chunk_size: usize) -> Vec<Bytes> {
    let mut chunks = Vec::with_capacity(num_chunks);
    let mut offset: usize = 0;
    for _ in 0..num_chunks {
        let mut buf = vec![0u8; chunk_size];
        offset = get_random_slice(&mut buf, offset);
        buf[chunk_size - 1] = 0x00;
        chunks.push(Bytes::from(buf));
    }
    chunks
}

pub async fn handle_get_time(
    stream: &mut Stream,
    command: &str,
) -> Result<(), Box<dyn Error + Send + Sync>> {
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

    let chunks = generate_chunks(64, chunk_size);
    let mut chunk_index = 0;

    let mut term_buf = vec![0u8; chunk_size];
    get_random_slice(&mut term_buf, 0);
    term_buf[chunk_size - 1] = 0x0FF;

    let start_time = Instant::now();
    // Send data until time expires
    while start_time.elapsed().as_secs() < duration {
        // Get next chunk from the array, cycling through all chunks
        // debug!("Sending chunk {}", chunk_index);
        let chunk = &chunks[chunk_index];
        stream.write_all(chunk).await?;
        total_bytes += chunk_size;

        // Move to next chunk, cycling back to start if needed
        chunk_index += 1;
        if chunk_index > 62 {
            chunk_index = 0;
        }
    }
    debug!("Sending last chunk");
    stream.write_all(&term_buf).await?;
    stream.flush().await?;
    let time_ns = start_time.elapsed().as_nanos();
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
    let time_response = format!("TIME {}\n", time_ns);
    stream.write_all(time_response.as_bytes()).await?;
    debug!("TIME response sent and flushed");

    Ok(())
}
