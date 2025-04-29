#[path = "../test_utils/mod.rs"]
mod test_utils;

use tokio::runtime::Runtime;
use log::{info, debug, trace};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use crate::test_utils::TestServer;
use std::time::Duration;
use env_logger;
use std::collections::VecDeque;
use tokio_tungstenite::tungstenite::Message;
use futures_util::{StreamExt, SinkExt};

const TEST_DURATION: u64 = 2; // 2 seconds
const CHUNK_SIZE: usize = 4096; // 4 KiB
const MIN_CHUNK_SIZE: usize = 4096; // 4 KiB
const MAX_CHUNK_SIZE: usize = 4194304; // 4 MiB

#[test]
fn test_handle_get_time() {
    // Setup logger
    let _ = env_logger::Builder::new()
        .filter_level(log::LevelFilter::Trace)
        .format_timestamp_millis()
        .format_module_path(false)
        .format_target(false)
        .try_init();

    let rt = Runtime::new().unwrap();
    
    rt.block_on(async {
        info!("Starting GETTIME test");
        
        // Create test server
        let server = TestServer::new(None, None).unwrap();
        info!("Test server created");
        
        // Connect to server
        let (mut stream, _) = server.connect_rmbtd().await.expect("Failed to connect to server");
        info!("Connected to server");
        
        // Read initial ACCEPT message after token validation
        let mut initial_accept = [0u8; 1024];
        let n = stream.read(&mut initial_accept).await.expect("Failed to read initial ACCEPT");
        let accept_str = String::from_utf8_lossy(&initial_accept[..n]);
        info!("Initial ACCEPT message: {}", accept_str);
        assert!(accept_str.contains("ACCEPT"), "Server should send initial ACCEPT message");
        
        // Test 1: Valid GETTIME command with default chunk size
        info!("Testing GETTIME with default chunk size");
        let command = format!("GETTIME {}\n", TEST_DURATION);
        stream.write_all(command.as_bytes()).await.expect("Failed to send GETTIME command");
        
        // Read and verify data stream
        let mut total_bytes = 0;
        let mut buffer = vec![0u8; CHUNK_SIZE];
        let start_time = std::time::Instant::now();
        let mut last_byte = 0u8;
        let mut is_full_chunk = false;
        let mut received_termination = false;
        
        while !received_termination && start_time.elapsed() < Duration::from_secs(TEST_DURATION + 1) {
            match stream.read(&mut buffer).await {
                Ok(n) => {
                    if n == 0 {
                        break;
                    }
                    total_bytes += n;
                    debug!("Received chunk size: {} bytes, total: {}, CHUNK_SIZE: {}", n, total_bytes, CHUNK_SIZE);
                    
                    // Verify chunk format and handle termination
                    is_full_chunk = n == CHUNK_SIZE;
                    last_byte = buffer[n-1];
                    debug!("is_full_chunk: {}, last_byte: {:#04x}", is_full_chunk, last_byte);
                    
                    if is_full_chunk {
                        debug!("Processing full chunk...");
                        if last_byte == 0xFF {
                            debug!("Received termination chunk");
                            received_termination = true;
                            // Send OK response to server immediately
                            debug!("Sending OK response");
                            stream.write_all(b"OK\n").await.expect("Failed to send OK response");
                            stream.flush().await.expect("Failed to flush OK response");
                            
                            // Read TIME response
                            let mut time_response = [0u8; 1024];
                            let n = stream.read(&mut time_response).await.expect("Failed to read TIME response");
                            let time_str = String::from_utf8_lossy(&time_response[..n]);
                            debug!("Received TIME response: '{}'", time_str);
                            
                            // Verify TIME response format
                            let time_parts: Vec<&str> = time_str.trim().split_whitespace().collect();
                            assert_eq!(time_parts.len(), 2, "TIME response should have 2 parts");
                            assert_eq!(time_parts[0], "TIME", "First part should be 'TIME'");
                            assert!(time_parts[1].parse::<u64>().is_ok(), "Second part should be a number");
                            
                            break;
                        } else {
                            assert_eq!(last_byte, 0x00, "Non-terminal chunk should end with 0x00");
                        }
                    }
                    
                    // Calculate and verify download speed
                    let elapsed_ns = start_time.elapsed().as_nanos() as f64;
                    let bytes_per_sec = (total_bytes as f64) / (elapsed_ns / 1e9);
                    debug!("Current download speed: {:.2} bytes/sec", bytes_per_sec);
                }
                Err(e) => {
                    panic!("Failed to read data: {}", e);
                }
            }
        }
        
        assert!(received_termination, "Did not receive termination chunk within timeout");
        
        // Send QUIT command
        info!("Sending QUIT command");
        stream.write_all(b"QUIT\n").await.expect("Failed to send QUIT");
        
        // Read ACCEPT response first
        let mut response = [0u8; 1024];
        let n = stream.read(&mut response).await.expect("Failed to read ACCEPT response");
        let accept_response = String::from_utf8_lossy(&response[..n]);
        debug!("Received ACCEPT response: '{}'", accept_response);
        assert!(accept_response.contains("ACCEPT"), "Server should respond with ACCEPT after QUIT");
        
        // Then read BYE response
        let mut response = [0u8; 1024];
        let n = stream.read(&mut response).await.expect("Failed to read BYE response");
        let bye_response = String::from_utf8_lossy(&response[..n]);
        debug!("Received BYE response: '{}'", bye_response);
        assert!(bye_response.contains("BYE"), "Server should respond with BYE");
        
        info!("GETTIME test completed successfully");
    });
}

#[tokio::test]
async fn test_handle_get_time_ws() {
    // Setup logger
    let _ = env_logger::Builder::new()
        .filter_level(log::LevelFilter::Trace)
        .format_timestamp_millis()
        .format_module_path(false)
        .format_target(false)
        .try_init();

    info!("Starting WebSocket GETTIME test");
    
    // Create test server
    let server = TestServer::new(None, None).unwrap();
    info!("Test server created");
    
    // Connect to server using WebSocket
    let (ws_stream, chunk_size) = server.connect_ws().await.expect("Failed to connect to server");
    let (mut write, mut read) = ws_stream.split();
    info!("Connected to server via WebSocket");
    
    // Test GETTIME command with default chunk size
    info!("Testing GETTIME with default chunk size");
    let command = format!("GETTIME {}\n", TEST_DURATION);
    write.send(Message::Text(command)).await.expect("Failed to send GETTIME command");
    
    // Read and verify data stream
    let mut total_bytes = 0;
    let mut buffer = vec![0u8; CHUNK_SIZE];
    let start_time = std::time::Instant::now();
    let mut last_byte = 0u8;
    let mut is_full_chunk = false;
    let mut received_termination = false;
    
    while !received_termination {
        match read.next().await {
            Some(Ok(message)) => {
                let data = message.into_data();
                let n = data.len();
                if n == 0 {
                    break;
                }
                total_bytes += n;
                debug!("Received chunk size: {} bytes, total: {}, CHUNK_SIZE: {}", n, total_bytes, CHUNK_SIZE);
                
                // Verify chunk format and handle termination
                is_full_chunk = n == CHUNK_SIZE;
                last_byte = data[n-1];
                debug!("is_full_chunk: {}, last_byte: {:#04x}", is_full_chunk, last_byte);
                
                if is_full_chunk {
                    debug!("Processing full chunk...");
                    if last_byte == 0xFF {
                        debug!("Received termination chunk");
                        received_termination = true;
                        // Send OK response to server immediately
                        debug!("Sending OK response");
                        write.send(Message::Text("OK\n".to_string())).await.expect("Failed to send OK response");
                        
                        // Read TIME response
                        let time_message = read.next().await.expect("Failed to read TIME response").expect("Failed to read TIME response");
                        let time_str = time_message.to_text().expect("TIME response is not text");
                        debug!("Received TIME response: '{}'", time_str);
                        
                        // Verify TIME response format
                        let time_parts: Vec<&str> = time_str.trim().split_whitespace().collect();
                        assert_eq!(time_parts.len(), 2, "TIME response should have 2 parts");
                        assert_eq!(time_parts[0], "TIME", "First part should be 'TIME'");
                        assert!(time_parts[1].parse::<u64>().is_ok(), "Second part should be a number");
                        
                        break;
                    } else {
                        assert_eq!(last_byte, 0x00, "Non-terminal chunk should end with 0x00");
                    }
                }
                
                // Calculate and verify download speed
                let elapsed_ns = start_time.elapsed().as_nanos() as f64;
                let bytes_per_sec = (total_bytes as f64) / (elapsed_ns / 1e9);
                debug!("Current download speed: {:.2} bytes/sec", bytes_per_sec);
            }
            Some(Err(e)) => {
                panic!("Failed to read data: {}", e);
            }
            None => break,
        }
    }
    
    assert!(received_termination, "Did not receive termination chunk");
    
    // Send QUIT command
    info!("Sending QUIT command");
    write.send(Message::Text("QUIT\n".to_string())).await.expect("Failed to send QUIT");
    
    // Read ACCEPT response first
    let accept_message = read.next().await.expect("Failed to read ACCEPT response").expect("Failed to read ACCEPT response");
    let accept_response = accept_message.to_text().expect("ACCEPT response is not text");
    debug!("Received ACCEPT response: '{}'", accept_response);
    assert!(accept_response.contains("ACCEPT"), "Server should respond with ACCEPT after QUIT");
    
    // Then read BYE response
    let bye_message = read.next().await.expect("Failed to read BYE response").expect("Failed to read BYE response");
    let bye_response = bye_message.to_text().expect("BYE response is not text");
    debug!("Received BYE response: '{}'", bye_response);
    assert!(bye_response.contains("BYE"), "Server should respond with BYE");
    
    // Close WebSocket connection
    write.close().await.expect("Failed to close WebSocket connection");
    info!("Closed WebSocket connection");
    
    info!("WebSocket GETTIME test completed successfully");
} 