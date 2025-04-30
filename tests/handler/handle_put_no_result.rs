#[path = "../test_utils/mod.rs"]
mod test_utils;

use tokio::runtime::Runtime;
use log::{info, debug};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use crate::test_utils::TestServer;
use fastrand::Rng;
use std::time::{Duration, Instant};
use env_logger;
use tokio_tungstenite::tungstenite::Message;
use futures_util::{StreamExt, SinkExt};
use tokio::time::{sleep, timeout};

const CHUNK_SIZE: usize = 4096; // 4 KiB
const TEST_DURATION: u64 = 2; // seconds for pre-test, as per specification
const MAX_CHUNKS: usize = 8; // Maximum number of chunks before increasing chunk size
const IO_TIMEOUT: Duration = Duration::from_secs(5); // Timeout for I/O operations

#[test]
fn test_handle_put_no_result_rmbt() {
    // Setup logger
    let _ = env_logger::Builder::new()
        .filter_level(log::LevelFilter::Debug)
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

        // Give server time to start
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Connect to server
        let (mut stream, _) = server.connect_rmbtd().await.expect("Failed to connect to server");
        info!("Connected to server");

        // Test PUTNORESULT command with increasing chunk sizes
        let mut current_chunks = 1;
        let mut current_chunk_size = CHUNK_SIZE;
        let mut rng = Rng::new();
        let start_time = Instant::now();

        while start_time.elapsed().as_secs() < TEST_DURATION {
            // Read ACCEPT response with timeout
            let mut accept_response = [0u8; 1024];
            let n = timeout(IO_TIMEOUT, stream.read(&mut accept_response))
                .await
                .expect("ACCEPT response timeout")
                .expect("Failed to read ACCEPT response");
            let accept_str = String::from_utf8_lossy(&accept_response[..n]);
            info!("Received ACCEPT response");
            assert!(accept_str.contains("ACCEPT"), "Server should respond with ACCEPT");

            // Send PUTNORESULT command with current chunk size
            let command = format!("PUTNORESULT {}\n", current_chunk_size);
            timeout(IO_TIMEOUT, stream.write_all(command.as_bytes()))
                .await
                .expect("Write command timeout")
                .expect("Failed to send PUTNORESULT command");
            info!("Sent PUTNORESULT command with chunk_size={}", current_chunk_size);
            sleep(Duration::from_millis(100)).await;

            // Read OK response with timeout
            let mut response = [0u8; 1024];
            let n = timeout(IO_TIMEOUT, stream.read(&mut response))
                .await
                .expect("OK response timeout")
                .expect("Failed to read OK response");
            let ok_response = String::from_utf8_lossy(&response[..n]);
            info!("Received OK response");
            assert!(ok_response.contains("OK"), "Server should respond with OK");

            // Send data chunks
            info!("Sending {} chunks of size {}", current_chunks, current_chunk_size);
            for i in 0..current_chunks {
                let mut chunk = vec![0u8; current_chunk_size];
                rng.fill(&mut chunk);

                // Set last byte: 0x00 for non-terminal chunks, 0xFF for terminal chunk
                if i == current_chunks - 1 {
                    chunk[current_chunk_size - 1] = 0xFF;
                } else {
                    chunk[current_chunk_size - 1] = 0x00;
                }
                sleep(Duration::from_millis(50)).await; //need to understand better

                timeout(IO_TIMEOUT, stream.write_all(&chunk))
                    .await
                    .expect("Write chunk timeout")
                    .expect("Failed to send chunk");
            }
            info!("All chunks sent successfully");
            sleep(Duration::from_millis(100)).await;


            // Read final TIME response with timeout
            let mut time_response = [0u8; 1024];
            let n = timeout(IO_TIMEOUT, stream.read(&mut time_response))
                .await
                .expect("TIME response timeout")
                .expect("Failed to read TIME response");
            let time_str = String::from_utf8_lossy(&time_response[..n]);
            info!("Received TIME response: {}", time_str.trim());

            // Verify TIME response format
            let time_parts: Vec<&str> = time_str.trim().split_whitespace().collect();
            assert_eq!(time_parts.len(), 2, "TIME response should have 2 parts");
            assert_eq!(time_parts[0], "TIME", "First part should be 'TIME'");
            assert!(time_parts[1].parse::<u64>().is_ok(), "Second part should be a number");

            // Double the number of chunks for next iteration
            current_chunks *= 2;

            // If we've reached the maximum number of chunks, increase chunk size
            if current_chunks > MAX_CHUNKS {
                current_chunks = 1;
                current_chunk_size *= 2;
            }
        }

        // Send QUIT command
        info!("Sending QUIT command");
        timeout(IO_TIMEOUT, stream.write_all(b"QUIT\n"))
            .await
            .expect("Write QUIT timeout")
            .expect("Failed to send QUIT");

        // Read ACCEPT response first
        let mut response = [0u8; 1024];
        let n = timeout(IO_TIMEOUT, stream.read(&mut response))
            .await
            .expect("Final ACCEPT timeout")
            .expect("Failed to read final ACCEPT response");
        let accept_response = String::from_utf8_lossy(&response[..n]);
        info!("Received final ACCEPT response");
        assert!(accept_response.contains("ACCEPT"), "Server should respond with ACCEPT after QUIT");

        // Then read BYE response
        let mut response = [0u8; 1024];
        let n = timeout(IO_TIMEOUT, stream.read(&mut response))
            .await
            .expect("BYE response timeout")
            .expect("Failed to read BYE response");
        let bye_response = String::from_utf8_lossy(&response[..n]);
        info!("Received BYE response");
        assert!(bye_response.contains("BYE"), "Server should respond with BYE");

        info!("PUTNORESULT test completed successfully");
    });
}

#[test]
fn test_handle_put_no_result_ws() {
    // Setup logger
    let _ = env_logger::Builder::new()
        .filter_level(log::LevelFilter::Debug)
        .format_timestamp_millis()
        .format_module_path(false)
        .format_target(false)
        .try_init();

    let rt = Runtime::new().unwrap();

    rt.block_on(async {
        info!("Starting WebSocket PUTNORESULT test");

        // Create test server
        let server = TestServer::new(None, None).unwrap();
        info!("Test server created");

        // Give server time to start
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Connect to server using WebSocket
        let (mut ws_stream, _) = server.connect_ws().await.expect("Failed to connect to server");
        info!("Connected to server via WebSocket");

        // Test PUTNORESULT command with increasing chunk sizes
        info!("Testing PUTNORESULT with increasing chunk sizes");
        let mut current_chunks = 1;
        let mut current_chunk_size = CHUNK_SIZE;
        let mut rng = Rng::new();
        let start_time = Instant::now();

        while start_time.elapsed().as_secs() < TEST_DURATION {
            // Read initial command list response
            let response = timeout(IO_TIMEOUT, ws_stream.next())
                .await
                .expect("Command list response timeout")
                .expect("Failed to read command list response")
                .expect("Failed to get message");
            let command_list = response.to_string();
            info!("Received command list: {}", command_list.trim());
            assert!(command_list.contains("ACCEPT"), "Server should send command list");

            // Send PUTNORESULT command with current chunk size
            let command = format!("PUTNORESULT {}\n", current_chunk_size);
            timeout(IO_TIMEOUT, ws_stream.send(Message::Text(command)))
                .await
                .expect("Write command timeout")
                .expect("Failed to send PUTNORESULT command");

            // Read OK response
            let response = timeout(IO_TIMEOUT, ws_stream.next())
                .await
                .expect("OK response timeout")
                .expect("Failed to read OK response")
                .expect("Failed to get message");
            let ok_response = response.to_string();
            debug!("Received OK response: '{}'", ok_response);
            assert!(ok_response.contains("OK"), "Server should respond with OK");

            // Send data chunks
            for i in 0..current_chunks {
                let mut chunk = vec![0u8; current_chunk_size];
                rng.fill(&mut chunk);

                // Set last byte: 0x00 for non-terminal chunks, 0xFF for terminal chunk
                if i == current_chunks - 1 {
                    chunk[current_chunk_size - 1] = 0xFF;
                    debug!("Sending terminal chunk with 0xFF");
                } else {
                    chunk[current_chunk_size - 1] = 0x00;
                    debug!("Sending non-terminal chunk with 0x00");
                }

                timeout(IO_TIMEOUT, ws_stream.send(Message::Binary(chunk)))
                    .await
                    .expect("Write chunk timeout")
                    .expect("Failed to send chunk");
                sleep(Duration::from_millis(50)).await;
            }

            // After terminal chunk, read final TIME response with retries
            let mut attempts = 0;
            let max_attempts = 5;

            while attempts < max_attempts {
                let response = timeout(IO_TIMEOUT, ws_stream.next())
                    .await
                    .expect("Final TIME response timeout")
                    .expect("Failed to read final TIME response")
                    .expect("Failed to get message");
                let response_str = response.to_string();
                
                // Check if we got a TIME response or command list
                if response_str.contains("TIME") {
                    info!("Received final TIME response: {}", response_str.trim());
                    
                    // Verify final TIME response format
                    let time_parts: Vec<&str> = response_str.trim().split_whitespace().collect();
                    assert_eq!(time_parts.len(), 2, "Final TIME response should have 2 parts");
                    assert_eq!(time_parts[0], "TIME", "First part should be 'TIME'");
                    assert!(time_parts[1].parse::<u64>().is_ok(), "Second part should be nanoseconds");
                    break;
                } else if response_str.contains("ACCEPT") {
                    // If we got command list instead of TIME, wait a bit and try again
                    info!("Received command list instead of TIME, retrying...");
                    sleep(Duration::from_millis(100)).await;
                    attempts += 1;
                    continue;
                } else {
                    panic!("Unexpected response: {}", response_str);
                }
            }

            if attempts >= max_attempts {
                panic!("Failed to receive final TIME response after {} attempts", max_attempts);
            }

            // Double the number of chunks for next iteration
            current_chunks *= 2;

            // If we've reached the maximum number of chunks, increase chunk size
            if current_chunks > MAX_CHUNKS {
                current_chunks = 1;
                current_chunk_size *= 2;
            }
        }

        // Send QUIT command
        info!("Sending QUIT command");
        timeout(IO_TIMEOUT, ws_stream.send(Message::Text("QUIT\n".to_string())))
            .await
            .expect("Write QUIT timeout")
            .expect("Failed to send QUIT");

        // Read ACCEPT response first
        let response = timeout(IO_TIMEOUT, ws_stream.next())
            .await
            .expect("Final ACCEPT timeout")
            .expect("Failed to read final ACCEPT response")
            .expect("Failed to get message");
        let accept_response = response.to_string();
        info!("Received final ACCEPT response");
        assert!(accept_response.contains("ACCEPT"), "Server should respond with ACCEPT");

        // Then read BYE response
        let response = timeout(IO_TIMEOUT, ws_stream.next())
            .await
            .expect("BYE response timeout")
            .expect("Failed to read BYE response")
            .expect("Failed to get message");
        let bye_response = response.to_string();
        info!("Received BYE response");
        assert!(bye_response.contains("BYE"), "Server should respond with BYE");

        // Close WebSocket connection
        ws_stream.close(None).await.expect("Failed to close WebSocket connection");
        info!("Closed WebSocket connection");

        info!("WebSocket PUTNORESULT test completed successfully");
    });
} 