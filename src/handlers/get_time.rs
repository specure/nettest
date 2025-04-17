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
use crate::utils::test_utils::TestUtils;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::connection_handler::{ConnectionHandler, Stream};
    use crate::server::server_config::ServerConfig;
    use crate::utils::test_utils::TestUtils;
    use crate::utils::token_validator::TokenValidator;
    use std::sync::Arc;
    use tokio::net::{TcpListener, TcpStream};
    use tokio::sync::Mutex;

    #[tokio::test]
    async fn test_get_time_with_handler() {
        let utils = TestUtils::new();
        let token_validator = Arc::new(TokenValidator::new(
            vec![utils.key.clone()],
            vec!["test_key".to_string()],
        ));

        // Create a TCP listener
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        // Create server config
        let config = Arc::new(ServerConfig {
            listen_addresses: vec![addr],
            ..Default::default()
        });

        // Create data buffer
        let data_buffer = Arc::new(Mutex::new(Vec::new()));

        // Create a task for the server
        let server_task = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut handler = ConnectionHandler::new(
                Stream::Plain(stream),
                config,
                token_validator,
                data_buffer,
            );
            handler.handle().await.unwrap();
        });

        // Create a task for the client
        let client_task = tokio::spawn(async move {
            let mut stream = TcpStream::connect(addr).await.unwrap();
            
            // Read greeting (RMBTv1.0)
            let mut buffer = vec![0; 1024];
            let n = stream.read(&mut buffer).await.unwrap();
            let greeting = String::from_utf8_lossy(&buffer[..n]);
            assert!(greeting.starts_with("RMBTv1.0"), "Expected RMBTv1.0 greeting, got: {}", greeting);
            
            // Read "ACCEPT TOKEN QUIT"
            let mut buffer = vec![0; 1024];
            let n = stream.read(&mut buffer).await.unwrap();
            let accept_msg = String::from_utf8_lossy(&buffer[..n]);
            assert!(accept_msg.contains("ACCEPT TOKEN QUIT"), "Expected ACCEPT TOKEN QUIT, got: {}", accept_msg);
            
            // Generate valid token components
            let uuid = TestUtils::generate_uuid();
            let timestamp = TestUtils::generate_timestamp().to_string();
            let key = TestUtils::generate_key();
            let token = utils.generate_token().await;

            // Send TOKEN command
            stream.write_all(format!("TOKEN {}\n", token).as_bytes()).await.unwrap();
            
            // Read OK response for token
            let mut buffer = vec![0; 1024];
            let n = stream.read(&mut buffer).await.unwrap();
            let response = String::from_utf8_lossy(&buffer[..n]);
            assert!(response.trim() == "OK", "Expected OK response for token, got: {}", response);
            
            // Read ACCEPT commands
            let mut buffer = vec![0; 1024];
            let n = stream.read(&mut buffer).await.unwrap();
            let accept_msg = String::from_utf8_lossy(&buffer[..n]);
            assert!(accept_msg.contains("ACCEPT GETCHUNKS GETTIME PUT PUTNORESULT PING QUIT"), 
                   "Expected ACCEPT commands, got: {}", accept_msg);
            
            // Send GETTIME command
            stream.write_all(b"GETTIME 1 1024\n").await.unwrap();
            
            // Read OK response
            let mut buffer = vec![0; 1024];
            let n = stream.read(&mut buffer).await.unwrap();
            let response = String::from_utf8_lossy(&buffer[..n]);
            assert!(response.trim() == "OK", "Expected OK response, got: {}", response);
            
            // Read chunks until termination byte
            let mut buffer = vec![0; 1024];
            let mut total_bytes = 0;
            let mut found_termination = false;
            
            loop {
                let n = stream.read(&mut buffer).await.unwrap();
                if n == 0 {
                    break;
                }
                total_bytes += n;
                
                // Check for termination byte (0xFF) in the last byte of the chunk
                if buffer[n-1] == 0xFF {
                    found_termination = true;
                    break;
                }
            }
            
            assert!(found_termination, "Did not find termination byte (0xFF)");
            
            // Send OK after receiving termination byte
            stream.write_all(b"OK\n").await.unwrap();
            
            // Read TIME response
            let mut response = vec![0; 1024];
            let n = stream.read(&mut response).await.unwrap();
            let response = String::from_utf8_lossy(&response[..n]);
            assert!(response.starts_with("TIME "), "Expected TIME response, got: {}", response);
            
            // Parse elapsed time
            let elapsed: f64 = response[5..].trim().parse().unwrap();
            assert!(elapsed > 0.0, "Elapsed time should be positive");
        });

        // Wait for both tasks to complete
        let (server_result, client_result) = tokio::join!(server_task, client_task);
        server_result.unwrap();
        client_result.unwrap();
    }
}
