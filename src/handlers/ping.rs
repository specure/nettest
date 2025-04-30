use crate::config::constants::{RESP_ERR, RESP_PONG, RESP_TIME};
use crate::stream::Stream;
use log::{debug, error};
use std::error::Error;
use std::time::Instant;

pub async fn handle_ping(stream: &mut Stream) -> Result<(), Box<dyn Error + Send + Sync>> {
    // Start time measurement
    let start_time = Instant::now();

    // Send PONG to client
    stream.write_all(RESP_PONG.as_bytes()).await?;
    stream.flush().await?;


    // Read OK response from client
    let mut buf = [0u8; 1024]; // Fixed buffer size as in C code
    let mut bytes_read = 0;

    while bytes_read < buf.len() {
        match stream.read(&mut buf[bytes_read..]).await? {
            0 => break, // EOF
            n => {
                bytes_read += n;
                if let Some(pos) = buf[..bytes_read].iter().position(|&b| b == b'\n') {
                    bytes_read = pos + 1;
                    break;
                }
            }
        }
    }

    let response = String::from_utf8_lossy(&buf[..bytes_read])
        .trim()
        .to_string();

    // End time measurement
    let elapsed_ns = start_time.elapsed().as_nanos() as u64;

    
    // Check client response
    if !response.trim().eq("OK") {
        error!("Expected OK from client, got: {}", response);
        stream.write_all(RESP_ERR.as_bytes()).await?;
        return Err("Invalid client response".into());
    } else {
        debug!("Received OK from client");
    }

    // Send execution time
    let time_response = format!("{} {}\n", RESP_TIME, elapsed_ns);
    stream.write_all(time_response.as_bytes()).await?;
    stream.flush().await?;

    debug!("PING completed in {} ns", elapsed_ns);

    Ok(())
}