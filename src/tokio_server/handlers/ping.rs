use crate::config::constants::{RESP_PONG};
use crate::tokio_server::stream::Stream;
use std::error::Error;
use quanta::Clock;

pub async fn handle_ping(stream: &mut Stream) -> Result<(), Box<dyn Error + Send + Sync>> {
    let clock = Clock::new();
    // Send PONG to client first
    stream.write_all(RESP_PONG.as_bytes()).await?;

    // Start time measurement after sending PONG

    // Read OK response from client
    let mut buf = [0u8; 1024];
   
    let start = clock.now();

    let n = stream.read(&mut buf).await?;
    let end: std::time::Duration = start.elapsed();

    if n == 0 {
        return Err("Connection closed by client".into());
    }

    let response = String::from_utf8_lossy(&buf[..n]).trim().to_string();
    if response != "OK" {
        return Err("Expected OK response".into());
    }


    // Send TIME response
    let time_response = format!("TIME {}\n", end.as_nanos());
    stream.write_all(time_response.as_bytes()).await?;

    Ok(())
}