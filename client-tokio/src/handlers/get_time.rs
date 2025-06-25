use std::time::{Duration, Instant};
use log::{debug, info};
use tokio::time::sleep;
use crate::config::constants::{CHUNK_SIZE, MIN_CHUNK_SIZE, MAX_CHUNK_SIZE, RESP_OK, RESP_ERR, RESP_TIME};
use crate::stream::Stream;
use crate::MeasurementResult;

const TEST_DURATION_NS: u64 = 10_000_000_000;


pub async fn handle_get_time(
    stream: &mut Stream,
    result: &mut MeasurementResult,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let command = format!(
        "GETTIME {} {}\n",
        TEST_DURATION_NS / 1_000_000_000,
        MAX_CHUNK_SIZE
    );
    stream.write_all(command.as_bytes()).await?;

    let chunk_size = MAX_CHUNK_SIZE;

    let mut total_bytes = 0;
    let mut buffer = vec![0u8; MAX_CHUNK_SIZE];
    let mut found_terminator = false;
    let start_time = Instant::now();

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
                Err(e) => {
                    debug!("Failed to read chunk: {}", e);
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
                debug!("Invalid chunk terminator: {}", terminator);
                break;
            }
        } else {
            debug!("Incomplete chunk read: {} bytes instead of {}", bytes_read, chunk_size);
            break;
        }
    }

    let elapsed_ns = start_time.elapsed().as_nanos() as u64;

    // debug!("Sleeping 10 seconds");
    //  sleep(Duration::from_secs(10)).await;

    info!(
        "PUTNORESULT completed: received {} bytes in {} ns, found_terminator: {}",
        total_bytes,
        elapsed_ns,
        found_terminator
    );
    result.download_bytes = total_bytes as u64;
    result.download_time = elapsed_ns;

    let speed = total_bytes as f64 / elapsed_ns as f64 * 1000000000.0;

    info!("Speed Gbit/s: {}", speed * 8.0 / 1024.0 / 1024.0 / 1024.0);

    //send ok
    stream.write_all(b"OK\n").await?;

    // // Send TIME response even if we didn't find a terminator
    // let time_response = format!("{} {}\n", RESP_TIME, elapsed_ns);
    // if let Err(e) = stream.write_all(time_response.as_bytes()).await {
    //     debug!("Failed to send TIME response: {}", e);
    // }

    Ok(())
} 