use std::time::{ Instant};
use log::{debug};
use crate::config::constants::{CHUNK_SIZE, MIN_CHUNK_SIZE, MAX_CHUNK_SIZE, RESP_OK, RESP_ERR, RESP_TIME};
use crate::stream::Stream;

pub async fn handle_put(
    stream: &mut Stream,
    command: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut parts = command.split_whitespace();
    let _ = parts.next(); // Skip "PUT"
    let chunk_size = if let Some(size) = parts.next() {
        match size.parse::<usize>() {
            Ok(size) if size >= MIN_CHUNK_SIZE && size <= MAX_CHUNK_SIZE => size,
            _ => {
                stream.write_all(RESP_ERR.as_bytes()).await?;
                return Err("Invalid chunk size".into());
            }
        }
    } else {
        CHUNK_SIZE
    };

    // Send OK response immediately after PUT command
    stream.write_all(RESP_OK.as_bytes()).await?;

    let start_time = Instant::now();
    // let mut last_time_ns = -1;
    let mut total_bytes = 0;
    let mut buffer = vec![0u8; chunk_size];
    let mut found_terminator = false;

    'read_chunks: loop {
        // Read exactly chunk_size bytes
        let mut bytes_read = 0;
        while bytes_read < chunk_size {
            match stream.read(&mut buffer[bytes_read..]).await {
                Ok(n) => {
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
        debug!("Read {} bytes", bytes_read);

        // Check if we got a complete chunk
        if bytes_read == chunk_size {

            debug!("Read {} bytes 2", bytes_read);
            total_bytes += bytes_read;
            
            // Check the last byte of the chunk for terminator
            let terminator = buffer[chunk_size - 1];
            // Send intermediate TIME response if needed
            let current_time_ns = start_time.elapsed().as_nanos() as i64;
            // if last_time_ns == -1  {
                debug!("Sending intermediate TIME response");
                let time_response = format!("{} {} BYTES {}\n", RESP_TIME, current_time_ns, total_bytes);
                if let Err(_) = stream.write_all(time_response.as_bytes()).await {
                    // error!("Failed to send intermediate TIME response: {}", e);
                    break;
                }
            // }

            // Check terminator
            if terminator == 0xFF {
                found_terminator = true;
                break;
            } else if terminator != 0x00 {
                // error!("Invalid chunk terminator: {}", terminator);
                break;
            }
        } else {
            // error!("Incomplete chunk read: {} bytes instead of {}", bytes_read, chunk_size);
            break;
        }
    }

    // Send final TIME response
    let total_time_ns = start_time.elapsed().as_nanos() as i64;
    let final_time_response = format!("{} {}\n", RESP_TIME, total_time_ns);
    if let Err(_) = stream.write_all(final_time_response.as_bytes()).await {
        // error!("Failed to send final TIME response: {}", e);
    }

    debug!(
        "PUT completed: received {} bytes in {} ns, found_terminator: {}",
        total_bytes,
        total_time_ns,
        found_terminator
    );

    Ok(())
}
