use crate::config::constants::{MAX_CHUNK_SIZE, MIN_CHUNK_SIZE, RESP_ERR};
use std::error::Error;
use std::time::Instant;
use log::{debug, error};
use crate::stream::Stream;
use bytes::Bytes;
use fastrand::Rng;

fn generate_chunks(num_chunks: usize, chunk_size: usize) -> Vec<Bytes> {
    let mut rng = Rng::new();
    let mut chunks = Vec::with_capacity(num_chunks);

    for i in 0..num_chunks {
        let mut buf = vec![0u8; chunk_size];
        rng.fill(&mut buf[..chunk_size - 1]); // random bytes
        buf[chunk_size - 1] = if i == num_chunks - 1 { 0xFF } else { 0x00 };
        chunks.push(Bytes::from(buf));
    }

    chunks
}

pub async fn handle_get_chunks(stream: &mut Stream, command: &str) -> Result<(), Box<dyn Error + Send + Sync>> {
    debug!("handle_get_chunks: starting");

    let parts: Vec<&str> = command[9..].trim().split_whitespace().collect();

    if parts.len() != 1 && parts.len() != 2 {
        stream.write_all(RESP_ERR.as_bytes()).await?;
        return Ok(());
    }

    let num_chunks = match parts[0].parse::<u64>() {
        Ok(n) => n,
        Err(_) => {
            stream.write_all(RESP_ERR.as_bytes()).await?;
            return Ok(());
        }
    };

    let chunk_size = if parts.len() == 1 {
        MIN_CHUNK_SIZE
    } else {
        match parts[1].parse::<usize>() {
            Ok(size) if size >= MIN_CHUNK_SIZE && size <= MAX_CHUNK_SIZE => size,
            _ => {
                stream.write_all(RESP_ERR.as_bytes()).await?;
                return Ok(());
            }
        }
    };

    if num_chunks < 1 {
        error!("Number of chunks must be at least 1");
        stream.write_all(RESP_ERR.as_bytes()).await?;
        return Ok(());
    }

    let chunks = generate_chunks(num_chunks as usize, chunk_size);

    let start_time = Instant::now();
    for chunk in &chunks {
        stream.write_all(chunk).await?;
    }
    stream.flush().await?;
    let time_ns = start_time.elapsed().as_nanos();
    let mut response = [0u8; 1024];
    let n = stream.read(&mut response).await?;
    let response_str = String::from_utf8_lossy(&response[..n]);

    if response_str.trim() != "OK" {
        error!("Expected OK response, got: {}", response_str);
        return Err("Invalid response from client".into());
    }

    let time_response = format!("TIME {}\n", time_ns);
    stream.write_all(time_response.as_bytes()).await?;
    debug!("TIME response sent and flushed");

    Ok(())
}
