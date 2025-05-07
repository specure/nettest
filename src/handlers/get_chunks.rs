use crate::config::constants::{MAX_CHUNK_SIZE, MIN_CHUNK_SIZE, RESP_ERR};
use std::error::Error;
use std::time::Instant;
use log::{debug, error};
use crate::stream::Stream;
use crate::utils::random_buffer::{get_buffer_size, get_random_slice};

pub async fn handle_get_chunks(stream: &mut Stream, command: &str) -> Result<(), Box<dyn Error + Send + Sync>> {
    debug!("handle_get_chunks: starting");

    // Parse command parts after GETCHUNKS
    let parts: Vec<&str> = command[9..].trim().split_whitespace().collect();

    // Validate command format exactly like in C code
    if parts.len() != 1 && parts.len() != 2 {
        stream.write_all(RESP_ERR.as_bytes()).await?;
        return Ok(());
    }

    // Parse number of chunks using strtoul-like parsing
    let num_chunks = match parts[0].parse::<u64>() {
        Ok(n) => n,
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

    // Check number of chunks
    if num_chunks < 1 {
        error!("Number of chunks must be at least 1");
        stream.write_all(RESP_ERR.as_bytes()).await?;
        return Ok(());
    }

    let mut total_bytes = 0;
    let start_time = Instant::now();
    let mut buffer = vec![0u8; chunk_size];

    let mut random_offset = 0;
    // Send specified number of chunks
    for i in 0..num_chunks {
        get_random_slice(&mut buffer[..chunk_size - 1], random_offset);
        random_offset = (random_offset + chunk_size - 1) % get_buffer_size();

        // Set last byte to 0xFF for the last chunk, 0x00 for others
        buffer[chunk_size - 1] = if i == num_chunks - 1 { 0xFF } else { 0x00 };

        // Send chunk
        stream.write_all(&buffer).await?;
        total_bytes += chunk_size;
    }


    // Wait for OK response from client
    let mut response = [0u8; 1024];
    let n = stream.read(&mut response).await?;
    let response_str = String::from_utf8_lossy(&response[..n]);

    // Calculate total time including OK response
    let time_ns = start_time.elapsed().as_nanos();

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
