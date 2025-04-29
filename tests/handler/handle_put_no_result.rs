#[path = "../test_utils/mod.rs"]
mod test_utils;

use tokio::runtime::Runtime;
use log::{info, debug, trace};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use crate::test_utils::TestServer;
use rand::RngCore;
use std::time::Duration;
use env_logger;
use tokio_tungstenite::tungstenite::Message;
use futures_util::{StreamExt, SinkExt};

const CHUNK_SIZE: usize = 4096; // 4 KiB
const MIN_CHUNK_SIZE: usize = 4096; // 4 KiB
const MAX_CHUNK_SIZE: usize = 4194304; // 4 MiB

#[test]
fn test_handle_put_no_result() {
    // Setup logger
    let _ = env_logger::Builder::new()
        .filter_level(log::LevelFilter::Trace)
        .format_timestamp_millis()
        .format_module_path(false)
        .format_target(false)
        .try_init();

    let rt = Runtime::new().unwrap();
    
    rt.block_on(async {
        info!("Starting PUTNORESULT test");
        
        // Create test server
        let server = TestServer::new(None, None).unwrap();
        info!("Test server created");
        
        // Connect to server
        let (mut stream, _) = server.connect_rmbtd().await.expect("Failed to connect to server");
        info!("Connected to server");
        
        // Test 1: Valid PUTNORESULT command with default chunk size
        info!("Testing PUTNORESULT with default chunk size");
        let mut current_chunks = 1;
        let mut total_bytes = 0;
        let mut rng = rand::thread_rng();
        
        while current_chunks <= 8 {
            // Read ACCEPT response
            let mut accept_response = [0u8; 1024];
            let n = stream.read(&mut accept_response).await.expect("Failed to read ACCEPT response");
            let accept_str = String::from_utf8_lossy(&accept_response[..n]);
            debug!("Received ACCEPT response: '{}'", accept_str);
            assert!(accept_str.contains("ACCEPT"), "Server should respond with ACCEPT");
           
            // Send PUTNORESULT command with current chunk size
            let command = format!("PUTNORESULT {}\n", CHUNK_SIZE * current_chunks);
            stream.write_all(command.as_bytes()).await.expect("Failed to send PUTNORESULT command");
            
            // Read OK response
            let mut response = [0u8; 1024];
            let n = stream.read(&mut response).await.expect("Failed to read OK response");
            let ok_response = String::from_utf8_lossy(&response[..n]);
            debug!("Received OK response: '{}'", ok_response);
            assert!(ok_response.contains("OK"), "Server should respond with OK");
            
            // Send data chunks
            for i in 0..current_chunks {
                let mut chunk = vec![0u8; CHUNK_SIZE];
                rng.fill_bytes(&mut chunk);
                
                // Set last byte: 0x00 for non-terminal chunks, 0xFF for terminal chunk
                if i == current_chunks - 1 {
                    chunk[CHUNK_SIZE - 1] = 0xFF;
                    debug!("Sending terminal chunk with 0xFF");
                } else {
                    chunk[CHUNK_SIZE - 1] = 0x00;
                    debug!("Sending non-terminal chunk with 0x00");
                }
                
                stream.write_all(&chunk).await.expect("Failed to send chunk");
                total_bytes += CHUNK_SIZE;
                debug!("Sent chunk {}, total bytes: {}", i + 1, total_bytes);
            }
            
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
            
            // Double the number of chunks for next iteration
            current_chunks *= 2;
        }
        
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
        
        info!("PUTNORESULT test completed successfully");
    });
}

#[tokio::test]
async fn test_handle_put_no_result_ws() {
    // Setup logger
    let _ = env_logger::Builder::new()
        .filter_level(log::LevelFilter::Trace)
        .format_timestamp_millis()
        .format_module_path(false)
        .format_target(false)
        .try_init();

    info!("Starting WebSocket PUTNORESULT test");
    
    // Create test server
    let server = TestServer::new(None, None).unwrap();
    info!("Test server created");
    
    // Connect to server using WebSocket
    let (ws_stream, _) = server.connect_ws().await.expect("Failed to connect to server");
    let (mut write, mut read) = ws_stream.split();
    info!("Connected to server via WebSocket");
    
    // Test PUTNORESULT command with default chunk size
    info!("Testing PUTNORESULT with default chunk size");
    let mut current_chunks = 1;
    let mut total_bytes = 0;
    let mut rng = rand::thread_rng();
    
    while current_chunks <= 8 {
        // Read ACCEPT response
        let accept_message = read.next().await.expect("Failed to read ACCEPT response").expect("Failed to read ACCEPT response");
        let accept_str = accept_message.to_text().expect("ACCEPT response is not text");
        debug!("Received ACCEPT response: '{}'", accept_str);
        assert!(accept_str.contains("ACCEPT"), "Server should respond with ACCEPT");
        
        // Send PUTNORESULT command with current chunk size
        let command = format!("PUTNORESULT {}\n", CHUNK_SIZE * current_chunks);
        write.send(Message::Text(command)).await.expect("Failed to send PUTNORESULT command");
        
        // Read OK response
        let ok_message = read.next().await.expect("Failed to read OK response").expect("Failed to read OK response");
        let ok_response = ok_message.to_text().expect("OK response is not text");
        debug!("Received OK response: '{}'", ok_response);
        assert!(ok_response.contains("OK"), "Server should respond with OK");
        
        // Send data chunks
        for i in 0..current_chunks {
            let mut chunk = vec![0u8; CHUNK_SIZE];
            rng.fill_bytes(&mut chunk);
            
            // Set last byte: 0x00 for non-terminal chunks, 0xFF for terminal chunk
            if i == current_chunks - 1 {
                chunk[CHUNK_SIZE - 1] = 0xFF;
                debug!("Sending terminal chunk with 0xFF");
            } else {
                chunk[CHUNK_SIZE - 1] = 0x00;
                debug!("Sending non-terminal chunk with 0x00");
            }
            
            write.send(Message::Binary(chunk)).await.expect("Failed to send chunk");
            total_bytes += CHUNK_SIZE;
            debug!("Sent chunk {}, total bytes: {}", i + 1, total_bytes);
        }
        
        // Read TIME response
        let time_message = read.next().await.expect("Failed to read TIME response").expect("Failed to read TIME response");
        let time_str = time_message.to_text().expect("TIME response is not text");
        debug!("Received TIME response: '{}'", time_str);
        
        // Verify TIME response format
        let time_parts: Vec<&str> = time_str.trim().split_whitespace().collect();
        assert_eq!(time_parts.len(), 2, "TIME response should have 2 parts");
        assert_eq!(time_parts[0], "TIME", "First part should be 'TIME'");
        assert!(time_parts[1].parse::<u64>().is_ok(), "Second part should be a number");
        
        // Double the number of chunks for next iteration
        current_chunks *= 2;
    }
    
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
    
    info!("WebSocket PUTNORESULT test completed successfully");
} 