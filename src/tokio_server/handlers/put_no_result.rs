use std::time::{Instant};
use log::{debug};
use crate::config::constants::{CHUNK_SIZE, MIN_CHUNK_SIZE, MAX_CHUNK_SIZE, RESP_OK, RESP_ERR, RESP_TIME};
use crate::tokio_server::stream::Stream;

pub async fn handle_put_no_result(
    stream: &mut Stream,
    command: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let parts: Vec<&str> = command.split_whitespace().collect();
    if parts.len() > 2 {
        stream.write_all(RESP_ERR.as_bytes()).await?;

        debug!("Invalid number of arguments for PUTNORESULT: {}", command);
        return Err("Invalid number of arguments for PUTNORESULT".into());
    }

    let chunk_size = if parts.len() == 2 {
        match parts[1].parse::<usize>() {
            Ok(size) if size >= MIN_CHUNK_SIZE && size <= MAX_CHUNK_SIZE => size,
            _ => {
                stream.write_all(RESP_ERR.as_bytes()).await?;
                debug!("Invalid chunk size for PUTNORESULT: {}", parts[1]);
                return Err("Invalid chunk size".into());
            }
        }
    } else {
        CHUNK_SIZE
    };

    debug!("Chunk size for PUTNORESULT: {}", chunk_size);

    // Send OK to client after chunk size validation
    stream.write_all(RESP_OK.as_bytes()).await?;

    let mut total_bytes = 0;
    let mut buffer = vec![0u8; chunk_size];
    let mut found_terminator = false;
    let start_time = Instant::now();

    debug!("Starting PUTNORESULT");
    'read_chunks: loop {
        // Read exactly chunk_size bytes
        let mut bytes_read = 0;
        while bytes_read < chunk_size {
            match stream.read(&mut buffer[bytes_read..]).await {
                Ok(n) => {
                    // debug!("Read {} put_no_result bytes", n);
                    if n == 0 {
                        debug!("Client closed connection during chunk read");
                        break 'read_chunks;
                    }
                    bytes_read += n;
                },
                Err(_) => {
                    // error!("Failed to read chunk: {}", e);
                    break 'read_chunks;
                }
            }
        }
        // Check if we got a complete chunk
        if bytes_read == chunk_size {
            total_bytes += bytes_read;
            // Check the last byte of the chunk for terminator
            let terminator = buffer[chunk_size - 1];
            if terminator == 0xFF {
                found_terminator = true;
                debug!("Found terminator: {}", terminator);
                break;
            } else if terminator != 0x00 {
                // error!("Invalid chunk terminator: {}", terminator);
                break;
            } else if terminator == 0x00 {
                debug!("Correct chunk terminator: {}", terminator);
            }
        } else {
            // error!("Incomplete chunk read: {} bytes instead of {}", bytes_read, chunk_size);
            break;
        }
    }

    let elapsed_ns = start_time.elapsed().as_nanos() as u64;

    // debug!("Sleeping 10 seconds");
    //  sleep(Duration::from_secs(10)).await;

    debug!(
        "PUTNORESULT completed: received {} bytes in {} ns, found_terminator: {}",
        total_bytes,
        elapsed_ns,
        found_terminator
    );

    // Send TIME response even if we didn't find a terminator
    let time_response = format!("{} {}\n", RESP_TIME, elapsed_ns);
    if let Err(e) = stream.write_all(time_response.as_bytes()).await {
        debug!("Failed to send TIME response: {}", e);
    }

    Ok(())
} 