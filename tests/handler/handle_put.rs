#[path = "../test_utils/mod.rs"]
mod test_utils;

use tokio::{runtime::Runtime, time::sleep};
use log::{info, debug, trace};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use crate::test_utils::{find_free_port, TestServer};
use rand::RngCore;
use std::time::{Duration, Instant};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;
use tokio_native_tls::TlsStream;
use tokio::net::TcpStream;
use std::error::Error;
use std::sync::Arc;
use futures_util::{SinkExt, StreamExt};
use tokio::time::timeout;

const TEST_DURATION: u64 = 5;
const CHUNK_SIZE: usize = 4096;
const MAX_CHUNKS: u32 = 8;
const IO_TIMEOUT: Duration = Duration::from_secs(10);

#[test]
fn test_handle_put_rmbt() {
    // Setup logger
    let _ = env_logger::Builder::new()
        .filter_level(log::LevelFilter::Trace)
        .format_timestamp_millis()
        .format_module_path(false)
        .format_target(false)
        .try_init();

    let rt = Runtime::new().unwrap();

    rt.block_on(async {
        info!("Starting PUT test");

        // Create test server
        let server = TestServer::new(None, None).unwrap();
        info!("Test server created");

        // Give server time to start
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Connect to server
        let (mut stream, _) = server.connect_rmbtd().await.expect("Failed to connect to server");
        info!("Connected to server");

        // Test PUT command with increasing chunk sizes
        let mut current_chunks = 1;
        let mut current_chunk_size = CHUNK_SIZE;
        let mut rng = rand::thread_rng();
        let start_time = Instant::now();

        while start_time.elapsed().as_secs() < TEST_DURATION {
            let mut accept_response = [0u8; 1024];
            let n = timeout(IO_TIMEOUT, stream.read(&mut accept_response))
                .await
                .expect("ACCEPT response timeout")
                .expect("Failed to read ACCEPT response");
            let accept_str = String::from_utf8_lossy(&accept_response[..n]);
            info!("Received ACCEPT response");
            assert!(accept_str.contains("ACCEPT"), "Server should respond with ACCEPT");

            // Send PUT command with current chunk size
            let command = format!("PUT {}\n", current_chunk_size);
            timeout(IO_TIMEOUT, stream.write_all(command.as_bytes()))
                .await
                .expect("Write command timeout")
                .expect("Failed to send PUT command");
            info!("Sent PUT command with chunk_size={}", current_chunk_size);

            // Read OK response with timeout
            let mut response = [0u8; 1024];
            let n = timeout(IO_TIMEOUT, stream.read(&mut response))
                .await
                .expect("OK response timeout")
                .expect("Failed to read OK response");
            let ok_response = String::from_utf8_lossy(&response[..n]);
            info!("Received OK response");
            assert!(ok_response.contains("OK"), "Server should respond with OK {}", ok_response);

            // Send data chunks
            info!("Sending {} chunks of size {}", current_chunks, current_chunk_size);
            for i in 0..current_chunks {
                let mut chunk = vec![0u8; current_chunk_size];
                rng.fill_bytes(&mut chunk);

                // Set last byte: 0x00 for non-terminal chunks, 0xFF for terminal chunk
                if i == current_chunks - 1 {
                    chunk[current_chunk_size - 1] = 0xFF;
                } else {
                    chunk[current_chunk_size - 1] = 0x00;
                }

                timeout(IO_TIMEOUT, stream.write_all(&chunk))
                    .await
                    .expect("Write chunk timeout")
                    .expect("Failed to send chunk");

                // After each chunk, read TIME BYTES response
                let mut response = [0u8; 1024];
                let n = timeout(IO_TIMEOUT, stream.read(&mut response))
                    .await
                    .expect("TIME BYTES response timeout")
                    .expect("Failed to read TIME BYTES response");
                let response_str = String::from_utf8_lossy(&response[..n]);
                info!("Received TIME BYTES response: {}", response_str.trim());
                
                // Verify TIME BYTES response format
                let time_parts: Vec<&str> = response_str.trim().split_whitespace().collect();
                assert_eq!(time_parts.len(), 4, "TIME BYTES response should have 4 parts");
                assert_eq!(time_parts[0], "TIME", "First part should be 'TIME'");
                assert!(time_parts[1].parse::<u64>().is_ok(), "Second part should be nanoseconds {}", time_parts[1]);
                assert_eq!(time_parts[2], "BYTES", "Third part should be 'BYTES'");
                assert!(time_parts[3].parse::<u64>().is_ok(), "Fourth part should be bytes {}", time_parts[3]);
                sleep(Duration::from_millis(50)).await;
                // After terminal chunk (0xFF), read final TIME response
                if i == current_chunks - 1 {
                    let mut response = [0u8; 1024];
                    let n = timeout(IO_TIMEOUT, stream.read(&mut response))
                        .await
                        .expect("Final TIME response timeout")
                        .expect("Failed to read final TIME response");
                    let response_str = String::from_utf8_lossy(&response[..n]);
                    info!("Received final TIME response: {}", response_str.trim());
                    
                    // Verify final TIME response format
                    let time_parts: Vec<&str> = response_str.trim().split_whitespace().collect();
                    assert_eq!(time_parts.len(), 2, "Final TIME response should have 2 parts");
                    assert_eq!(time_parts[0], "TIME", "First part should be 'TIME'");
                    assert!(time_parts[1].parse::<u64>().is_ok(), "Second part should be nanoseconds {}", time_parts[1]);
                }
            }
            info!("All chunks sent successfully");

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
        assert!(accept_response.contains("ACCEPT"), "Server should respond with ACCEPT");

        // Then read BYE response
        let mut response = [0u8; 1024];
        let n = timeout(IO_TIMEOUT, stream.read(&mut response))
            .await
            .expect("BYE response timeout")
            .expect("Failed to read BYE response");
        let bye_response = String::from_utf8_lossy(&response[..n]);
        info!("Received BYE response");
        assert!(bye_response.contains("BYE"), "Server should respond with BYE");

        info!("PUT test completed successfully");
    });
}

#[test]
fn test_handle_put_ws() {
    // Setup logger
    let _ = env_logger::Builder::new()
        .filter_level(log::LevelFilter::Trace)
        .format_timestamp_millis()
        .format_module_path(false)
        .format_target(false)
        .try_init();

    let rt = Runtime::new().unwrap();

    rt.block_on(async {
        info!("Starting WebSocket PUT test");

        // Create test server with WebSocket enabled
        let server = TestServer::new(None, Some(443)).unwrap();
        info!("Test server created with WebSocket enabled");

        // Give server time to start
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Connect to server via WebSocket
        let (mut ws_stream, _) = server.connect_ws().await.expect("Failed to connect to server");
        info!("Connected to server via WebSocket");

        // Test PUT command with increasing chunk sizes
        let mut current_chunks = 1;
        let mut current_chunk_size = CHUNK_SIZE;
        let mut rng = rand::thread_rng();
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

            // Send PUT command with current chunk size
            let command = format!("PUT {}\n", current_chunk_size);
            timeout(IO_TIMEOUT, ws_stream.send(Message::Text(command)))
                .await
                .expect("Write command timeout")
                .expect("Failed to send PUT command");
            info!("Sent PUT command with chunk_size={}", current_chunk_size);

            // Read OK response with timeout
            let response = timeout(IO_TIMEOUT, ws_stream.next())
                .await
                .expect("OK response timeout")
                .expect("Failed to read OK response")
                .expect("Failed to get message");
            let ok_response = response.to_string();
            info!("Received OK response");
            assert!(ok_response.contains("OK"), "Server should respond with OK {}", ok_response);

            // Send data chunks
            info!("Sending {} chunks of size {}", current_chunks, current_chunk_size);
            for i in 0..current_chunks {
                let mut chunk = vec![0u8; current_chunk_size];
                rng.fill_bytes(&mut chunk);

                // Set last byte: 0x00 for non-terminal chunks, 0xFF for terminal chunk
                if i == current_chunks - 1 {
                    chunk[current_chunk_size - 1] = 0xFF;
                } else {
                    chunk[current_chunk_size - 1] = 0x00;
                }

                timeout(IO_TIMEOUT, ws_stream.send(Message::Binary(chunk)))
                    .await
                    .expect("Write chunk timeout")
                    .expect("Failed to send chunk");

                // After each chunk, read TIME BYTES response
                let response = timeout(IO_TIMEOUT, ws_stream.next())
                    .await
                    .expect("TIME BYTES response timeout")
                    .expect("Failed to read TIME BYTES response")
                    .expect("Failed to get message");
                let response_str = response.to_string();
                info!("Received TIME BYTES response: {}", response_str.trim());
                
                // Verify TIME BYTES response format
                let time_parts: Vec<&str> = response_str.trim().split_whitespace().collect();
                assert_eq!(time_parts.len(), 4, "TIME BYTES response should have 4 parts");
                assert_eq!(time_parts[0], "TIME", "First part should be 'TIME'");
                assert!(time_parts[1].parse::<u64>().is_ok(), "Second part should be nanoseconds {}", time_parts[1]);
                assert_eq!(time_parts[2], "BYTES", "Third part should be 'BYTES'");
                assert!(time_parts[3].parse::<u64>().is_ok(), "Fourth part should be bytes {}", time_parts[3]);

                sleep(Duration::from_millis(100)).await;

                // After terminal chunk (0xFF), read final TIME response
                if i == current_chunks - 1 {
                    let response = timeout(IO_TIMEOUT, ws_stream.next())
                        .await
                        .expect("Final TIME response timeout")
                        .expect("Failed to read final TIME response")
                        .expect("Failed to get message");
                    let response_str = response.to_string();
                    info!("Received final TIME response: {}", response_str.trim());
                    
                    // Verify final TIME response format
                    let time_parts: Vec<&str> = response_str.trim().split_whitespace().collect();
                    assert_eq!(time_parts.len(), 2, "Final TIME response should have 2 parts");
                    assert_eq!(time_parts[0], "TIME", "First part should be 'TIME'");
                    assert!(time_parts[1].parse::<u64>().is_ok(), "Second part should be nanoseconds {}", time_parts[1]);
                }
            }
            info!("All chunks sent successfully");

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

        info!("WebSocket PUT test completed successfully");
    });
} 