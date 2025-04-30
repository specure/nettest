use std::time::{ Instant};
use log::{debug};
use crate::config::constants::{CHUNK_SIZE, MIN_CHUNK_SIZE, MAX_CHUNK_SIZE, RESP_OK, RESP_TIME};
use crate::stream::Stream;

pub async fn handle_put(
    stream: &mut Stream,
    command: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut parts = command.split_whitespace();
    let _ = parts.next(); // Skip "PUT"
    let chunk_size = if let Some(size) = parts.next() {
        size.parse::<usize>()?
    } else {
        CHUNK_SIZE
    };

    if chunk_size < MIN_CHUNK_SIZE || chunk_size > MAX_CHUNK_SIZE {
        return Err("Invalid chunk size".into());
    }

    // Send OK response immediately after PUT command
    stream.write_all(RESP_OK.as_bytes()).await?;

    let start_time = Instant::now();
    let mut last_time_ns = -1;
    let mut total_bytes = 0;
    let mut last_byte = 0;

    while last_byte != 0xFF {
        let mut chunk = vec![0u8; chunk_size];
        let bytes_read = stream.read(&mut chunk).await?;
        if bytes_read == 0 {
            break;
        }

        total_bytes += bytes_read;
        last_byte = chunk[bytes_read - 1];

        let current_time_ns = start_time.elapsed().as_nanos() as i64;
        if last_time_ns == -1 || (current_time_ns - last_time_ns > 1_000_000) {
            last_time_ns = current_time_ns;
            let time_response = format!("{} {} BYTES {}\n", RESP_TIME, current_time_ns, total_bytes);
            if let Err(e) = stream.write_all(time_response.as_bytes()).await {
                debug!("Failed to send TIME response: {}", e);
                break;
            }
        }
    }

    let total_time_ns = start_time.elapsed().as_nanos() as i64;
    let final_time_response: String = format!("{} {}\n", RESP_TIME, total_time_ns);
    stream.write_all(final_time_response.as_bytes()).await?;
    Ok(())
}
