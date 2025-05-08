use crate::config::constants::{RESP_PONG};
use crate::stream::Stream;
use std::error::Error;
use std::time::Instant;

pub async fn handle_ping(stream: &mut Stream) -> Result<(), Box<dyn Error + Send + Sync>> {
    // Send PONG to client first
    stream.write_all(RESP_PONG.as_bytes()).await?;

    // Start time measurement after sending PONG

    // Read OK response from client
    let mut buf = [0u8; 1024];
    let start_time = Instant::now();

    let n = stream.read(&mut buf).await?;
    if n == 0 {
        return Err("Connection closed by client".into());
    }
    let time_ns = start_time.elapsed().as_nanos();

    let response = String::from_utf8_lossy(&buf[..n]).trim().to_string();
    if response != "OK" {
        return Err("Expected OK response".into());
    }

    // Calculate elapsed time

    // Send TIME response
    let time_response = format!("TIME {}\n", time_ns);
    stream.write_all(time_response.as_bytes()).await?;

    Ok(())
}