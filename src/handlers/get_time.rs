use crate::config::constants::{MAX_CHUNK_SIZE, MIN_CHUNK_SIZE, RESP_ERR, RESP_OK};
use crate::server::connection_handler::{ConnectionHandler, Stream};
use rand::rngs::StdRng;
use rand::{RngCore, SeedableRng};
use std::error::Error;
use std::time::Instant;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use std::pin::Pin;
use std::io;
use std::task::{Context, Poll};
use tokio::io::ReadBuf;
use crate::utils::token_validator::TokenValidator;
use tokio::time;
use log::{debug, error, info};
use tokio::time::timeout;
use std::time::Duration;
use crate::server::server_config::ServerConfig;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;

pub async fn handle_get_time(stream: &mut Stream, command: &str) -> Result<(), Box<dyn Error + Send + Sync>> {
    debug!("handle_get_time: starting");
    let start = Instant::now();

    // Check command name using strncmp-like comparison
    if !command.starts_with("GETTIME") || command.len() < 7 {
        stream.write_all(RESP_ERR.as_bytes()).await?;
        return Ok(());
    }

    // Parse command parts after GETTIME
    let parts: Vec<&str> = command[7..].trim().split_whitespace().collect();

    // Validate command format exactly like in C code
    if parts.len() != 2 && parts.len() != 3 {
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

    // Parse and validate chunk size
    let chunk_size = if parts.len() == 2 {
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

    // Send OK response
    println!("handle_get_time: sending OK response");
    stream.write_all(RESP_OK.as_bytes()).await?;
    stream.flush().await?;

    // Generate random data for chunks
    let mut rng = StdRng::from_entropy();
    let mut data = vec![0u8; chunk_size];
    rng.fill_bytes(&mut data);

    // Send chunks for specified duration
    let mut total_sent = 0;
    while start.elapsed().as_secs() < duration {
        debug!("Sending chunk {} of {}", total_sent + 1, chunk_size);
        stream.write_all(&data).await?;
        stream.flush().await?;
        debug!("Chunk {} sent and flushed", total_sent + 1);
        total_sent += chunk_size;
    }

    debug!("All chunks sent, sending termination byte");
    // Send final chunk with termination byte (0xFF)
    data[chunk_size - 1] = 0xFF;
    stream.write_all(&data).await?;
    stream.flush().await?;
    debug!("Termination byte sent and flushed");

    // Wait for OK from client
    debug!("Waiting for OK response");
    let mut response = [0u8; 1024];
    let n = stream.read(&mut response).await?;
    let response_str = String::from_utf8_lossy(&response[..n]);
    debug!("Received response: '{}' ({} bytes)", response_str, n);
    if response_str.trim() != "OK" {
        error!("Expected 'OK', got '{}'", response_str);
        return Err("Expected OK response".into());
    }

    debug!("OK response received, calculating elapsed time");
    // Send TIME response
    let elapsed = start.elapsed().as_micros();
    let response = format!("TIME {}", elapsed);
    debug!("Sending TIME response: {}", response);
    stream.write_all(response.as_bytes()).await?;
    stream.flush().await?;
    debug!("TIME response sent and flushed");

    Ok(())
}

