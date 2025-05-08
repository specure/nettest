#[path = "../test_utils/mod.rs"]
mod test_utils;

/// This test file implements the RMBT protocol's downlink pre-test phase using the GETCHUNKS command.
/// It verifies the server's ability to send data chunks of specified sizes, following the RMBT
/// specification for progressive chunk size increases. The test includes both plain TCP and WebSocket
/// implementations, validating proper chunk termination (0x00 for intermediate chunks, 0xFF for final),
/// server timing responses, and connection handling. Tests run for a specified duration, doubling
/// chunk counts until reaching a maximum, then increasing chunk sizes according to the RMBT protocol.

use tokio::runtime::Runtime;
use log::{info, debug};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use crate::test_utils::TestServer;
use std::time::Duration;
use env_logger;
use tokio::time::sleep;
use tokio_tungstenite::tungstenite::{Message};
use futures_util::{SinkExt, StreamExt};

// Constants from RMBT specification
const MIN_CHUNK_SIZE: u32 = 4096;  // 4KB
const MAX_CHUNK_SIZE: u32 = 4194304;  // 4MB
const MAX_CHUNKS: usize = 8;  // Maximum number of chunks before increasing size
const TEST_DURATION: u64 = 7;  // 7 seconds as per spec

#[test]
fn test_handle_get_chunks_r() {
    let _ = env_logger::Builder::new()
        .filter_level(log::LevelFilter::Debug)
        .format_timestamp_millis()
        .format_module_path(false)
        .format_target(false)
        .try_init();

    let rt = Runtime::new().unwrap();
    
    rt.block_on(async {
        let server = TestServer::new(None, None).unwrap();
        
        let (mut stream, initial_chunk_size) = server.connect_rmbtd().await.expect("Failed to connect to server");
        
        let mut chunks = 1;
        let mut chunk_size = initial_chunk_size;
        let mut total_bytes = 0;
        let mut bytes_per_sec = Vec::new();
        
        let start_time = std::time::Instant::now();
        let test_duration = Duration::from_secs(TEST_DURATION);
        
        while start_time.elapsed() < test_duration {
            let getchunks_cmd = format!("GETCHUNKS {} {}\n", chunks, chunk_size);
            debug!("Sending GETCHUNKS command: chunks={}, chunk_size={}", chunks, chunk_size);
            stream.write_all(getchunks_cmd.as_bytes())
                .await
                .expect("Failed to send GETCHUNKS");
            stream.flush().await.expect("Failed to flush GETCHUNKS command");
            
            let mut found_terminator = false;
            let mut chunks_received = 0;
            let mut total_bytes_read = 0;
            let mut last_byte = 0u8;
            let chunk_start_time = std::time::Instant::now();
            
            while !found_terminator {
                let mut buf = vec![0u8; chunk_size as usize];
                
                match stream.read(&mut buf).await {
                    Ok(n) => {
                        if n == 0 {
                            panic!("Connection closed before receiving full chunk");
                        }
                        
                        total_bytes_read += n;
                        total_bytes += n;
                        last_byte = buf[n - 1];
                        
                        debug!("Read {} bytes, total: {}, last byte: 0x{:02X}", 
                               n, total_bytes_read, last_byte);
                        
                        if last_byte == 0xFF {
                            found_terminator = true;
                            chunks_received += 1;
                            debug!("Found terminator byte 0xFF, received chunk {}/{}", chunks_received, chunks);
                        } else if last_byte == 0x00 {
                            chunks_received += 1;
                            debug!("Received chunk {}/{}", chunks_received, chunks);
                        }
                    }
                    Err(e) => {
                        panic!("Failed to read chunk: {}", e);
                    }
                }
            }
            
            assert_eq!(chunks_received, chunks, "Did not receive all chunks");
            assert!(found_terminator, "Did not find terminator byte");
            
            // Send OK only after receiving all chunks
            stream.write_all(b"OK").await.expect("Failed to send OK");
            stream.flush().await.expect("Failed to flush OK");
            
            let mut response = [0u8; 1024];
            let n = stream.read(&mut response).await.expect("Failed to read TIME response");
            let time_response = String::from_utf8_lossy(&response[..n]);
            debug!("Received TIME response: {}", time_response);
            assert!(time_response.contains("TIME"), "Server should respond with TIME");
            
            let time_str = time_response.trim().split_whitespace().nth(1).unwrap();
            let time_ns = time_str.parse::<u64>().expect("Failed to parse time");
            assert!(time_ns > 0, "Time should be positive");
            
            // Calculate speed using server's time
            let bytes_per_second = (total_bytes_read as f64 * 1_000_000_000.0 / time_ns as f64) as u64;
            bytes_per_sec.push(bytes_per_second);
            
            // Calculate optimal chunk size based on current performance
            if chunks >= MAX_CHUNKS {
                chunks = 1;
                // Calculate new chunk size based on average bytes per second
                let avg_bytes_per_sec: u64 = bytes_per_sec.iter().sum::<u64>() / bytes_per_sec.len() as u64;
                // Target 1 chunk every 20ms (50 chunks per second)
                let target_chunk_size = (avg_bytes_per_sec / 50) as u32;
                chunk_size = target_chunk_size.clamp(MIN_CHUNK_SIZE, MAX_CHUNK_SIZE);
                debug!("Increasing chunk size to {} based on performance", chunk_size);
            } else {
                chunks *= 2;
            }
        }
        
        assert!(total_bytes > 0, "Should receive some data");
        debug!("Test completed: received {} bytes in total", total_bytes);
        
        // Print final statistics
        let avg_bytes_per_sec: u64 = bytes_per_sec.iter().sum::<u64>() / bytes_per_sec.len() as u64;
        let avg_mbps = (avg_bytes_per_sec as f64 * 8.0) / 1_000_000.0;
        debug!("Average speed: {:.2} Mbps", avg_mbps);
    });
}

#[tokio::test]
async fn test_handle_get_chunks_ws() {
    let _ = env_logger::Builder::new()
        .filter_level(log::LevelFilter::Debug)
        .format_timestamp_millis()
        .format_module_path(false)
        .format_target(false)
        .try_init();

    let server = TestServer::new(None, None).unwrap();

    let (ws_stream, initial_chunk_size) = server.connect_ws().await.expect("Failed to connect to server");
    let (mut write, mut read) = ws_stream.split();

    let mut chunks = 1;
    let mut chunk_size = initial_chunk_size;
    let mut total_bytes = 0;
    let mut bytes_per_sec = Vec::new();
    
    let start_time = std::time::Instant::now();
    let test_duration = Duration::from_secs(TEST_DURATION);
    
    while start_time.elapsed() < test_duration {
        let getchunks_cmd = format!("GETCHUNKS {} {}\n", chunks, chunk_size);
        debug!("Sending GETCHUNKS command: chunks={}, chunk_size={}", chunks, chunk_size);
        write.send(Message::Text(getchunks_cmd)).await.expect("Failed to send GETCHUNKS");

        let mut found_terminator = false;
        let mut chunks_received = 0;
        let mut total_bytes_read = 0;
        let mut last_byte = 0u8;
        let chunk_start_time = std::time::Instant::now();
        
        while !found_terminator {
            match read.next().await {
                Some(Ok(message)) => {
                    let data = match message {
                        Message::Binary(data) => data,
                        Message::Text(text) => text.into_bytes(),
                        _ => panic!("Unexpected message type"),
                    };
                    
                    let n = data.len();
                    if n == 0 {
                        panic!("Connection closed before receiving full chunk");
                    }
                    
                    total_bytes_read += n;
                    total_bytes += n;
                    last_byte = data[n - 1];
                    
                    debug!("Read {} bytes, total: {}, last byte: 0x{:02X}", 
                           n, total_bytes_read, last_byte);
                    
                    if last_byte == 0xFF {
                        found_terminator = true;
                        chunks_received += 1;
                        debug!("Found terminator byte 0xFF, received chunk {}/{}", chunks_received, chunks);
                    } else if last_byte == 0x00 {
                        chunks_received += 1;
                        debug!("Received chunk {}/{}", chunks_received, chunks);
                    }
                }
                Some(Err(e)) => {
                    panic!("Failed to read chunk: {}", e);
                }
                None => {
                    panic!("Connection closed before receiving all chunks");
                }
            }
        }
        
        assert_eq!(chunks_received, chunks, "Did not receive all chunks");
        assert!(found_terminator, "Did not find terminator byte");
        
        // Send OK only after receiving all chunks
        write.send(Message::Text("OK".to_string())).await.expect("Failed to send OK");
        
        let time_response = read.next().await.expect("Failed to read TIME response").expect("Failed to read TIME response");
        let time_text = time_response.to_text().expect("TIME response is not text");
        debug!("Received TIME response: {}", time_text);
        assert!(time_text.contains("TIME"), "Server should respond with TIME");
        
        let time_str = time_text.trim().split_whitespace().nth(1).unwrap();
        let time_ns = time_str.parse::<u64>().expect("Failed to parse time");
        assert!(time_ns > 0, "Time should be positive");
        
        // Calculate speed using server's time
        let bytes_per_second = (total_bytes_read as f64 * 1_000_000_000.0 / time_ns as f64) as u64;
        bytes_per_sec.push(bytes_per_second);
        
        // Calculate optimal chunk size based on current performance
        if chunks >= MAX_CHUNKS {
            chunks = 1;
            // Calculate new chunk size based on average bytes per second
            let avg_bytes_per_sec: u64 = bytes_per_sec.iter().sum::<u64>() / bytes_per_sec.len() as u64;
            // Target 1 chunk every 20ms (50 chunks per second)
            let target_chunk_size = (avg_bytes_per_sec / 50) as u32;
            chunk_size = target_chunk_size.clamp(MIN_CHUNK_SIZE, MAX_CHUNK_SIZE);
            debug!("Increasing chunk size to {} based on performance", chunk_size);
        } else {
            chunks *= 2;
        }
    }
    
    assert!(total_bytes > 0, "Should receive some data");
    debug!("Test completed: received {} bytes in total", total_bytes);
    
    // Print final statistics
    let avg_bytes_per_sec: u64 = bytes_per_sec.iter().sum::<u64>() / bytes_per_sec.len() as u64;
    let avg_mbps = (avg_bytes_per_sec as f64 * 8.0) / 1_000_000.0;
    debug!("Average speed: {:.2} Mbps", avg_mbps);
} 