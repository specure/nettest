use crate::config::constants::{MAX_CHUNK_SIZE, MIN_CHUNK_SIZE, RESP_ERR, RESP_OK};
use std::error::Error;
use std::time::{Duration, Instant};
use log::{debug, error};
use crate::stream::Stream;
use fastrand::Rng;

const MAX_SECONDS: u64 = 3600; // Maximum test duration in seconds, matching C implementation

pub async fn handle_get_time(stream: &mut Stream, command: &str) -> Result<(), Box<dyn Error + Send + Sync>> {
    debug!("handle_get_time: starting");

    // Parse command parts after GETTIME
    let parts: Vec<&str> = command[7..].trim().split_whitespace().collect();

    // Validate command format - 1 or 2 parts (duration and optional chunk size)
    if parts.is_empty() || parts.len() > 2 {
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

    // Check duration constraints like in C code
    if duration == 0 || duration > MAX_SECONDS {
        error!("Duration must be between 1 and {} seconds", MAX_SECONDS);
        stream.write_all(RESP_ERR.as_bytes()).await?;
        return Ok(());
    }

    // Parse and validate chunk size if provided (2nd part)
    let chunk_size = if parts.len() == 2 {
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
    } else {
        MIN_CHUNK_SIZE
    };

    // Send initial OK response
    stream.write_all(RESP_OK.as_bytes()).await?;
    stream.flush().await?;

    let start_time = Instant::now();
    let duration_ns = Duration::from_secs(duration);
    let mut buffer = vec![0u8; chunk_size];
    let mut rng = Rng::new();
    let mut total_bytes = 0usize;
    let mut last_intermediate_time = Instant::now();

    // Main test loop - send data until time expires
    loop {
        // Fill buffer with random data using fastrand
        rng.fill(&mut buffer[..chunk_size - 1]);
        
        // Check if we've exceeded the duration
        let elapsed = start_time.elapsed();
        
        // Set terminator byte - 0xFF for last chunk, 0x00 for others
        buffer[chunk_size - 1] = if elapsed >= duration_ns {
            0xFF // Last chunk
        } else {
            0x00 // Intermediate chunk
        };
        
        // Send chunk
        match stream.write_all(&buffer).await {
            Ok(_) => {
                total_bytes += chunk_size;
                
                // Send intermediate TIME response every 200ms
                let now = Instant::now();
                if now.duration_since(last_intermediate_time) >= Duration::from_millis(200) {
                    let elapsed_ns = elapsed.as_nanos();
                    let time_response = format!("TIME {} BYTES {}\n", elapsed_ns, total_bytes);
                    stream.write_all(time_response.as_bytes()).await?;
                    stream.flush().await?;
                    last_intermediate_time = now;
                }
                
                if buffer[chunk_size - 1] == 0xFF {
                    break;
                }
            }
            Err(e) => {
                error!("Error writing chunk: {}", e);
                stream.write_all(RESP_ERR.as_bytes()).await?;
                return Ok(());
            }
        }
    }

    debug!("Total bytes sent: {}", total_bytes);

    // Wait for OK response from client
    let mut response = [0u8; 1024];
    let n = stream.read(&mut response).await?;
    let response_str = String::from_utf8_lossy(&response[..n]);
        
    if response_str.trim() != "OK" {
        error!("Expected OK response, got: {}", response_str);
        stream.write_all(RESP_ERR.as_bytes()).await?;
        return Ok(());
    }

    // Send final TIME response with nanoseconds
    let elapsed = start_time.elapsed();
    let time_ns = elapsed.as_nanos();
    let time_response = format!("TIME {}\n", time_ns);
    stream.write_all(time_response.as_bytes()).await?;
    stream.flush().await?;
    debug!("TIME response sent and flushed. Total time: {}ns, bytes: {}", time_ns, total_bytes);

    Ok(())
}

