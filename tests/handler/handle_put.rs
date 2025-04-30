#[path = "../test_utils/mod.rs"]
mod test_utils;

/// This test file implements the RMBT protocol's uplink measurement phase using the PUT command.
/// It verifies the server's ability to receive and process data uploads with intermediate result
/// reporting, following the RMBT specification for timing and chunk handling. The test includes
/// both plain TCP and WebSocket implementations, ensuring proper data transmission, timing
/// measurements, and intermediate progress reporting. It validates chunk termination, server
/// responses, and connection cleanup while maintaining protocol compliance.

use tokio::{runtime::Runtime};
use log::{info, debug};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use crate::test_utils::TestServer;
use fastrand::Rng;
use std::time::{Duration, Instant};
use tokio_tungstenite::tungstenite::Message;
use futures_util::{SinkExt, StreamExt};
use tokio::time::{timeout, sleep};

// Constants matching RMBT specification and client implementation
const TEST_DURATION: u64 = 5; // Test duration in seconds
const CHUNK_SIZE: usize = 4096; // Initial chunk size (4 KiB)
const MAX_CHUNKS: u32 = 8; // Maximum number of chunks before increasing size
const IO_TIMEOUT: Duration = Duration::from_secs(10); // I/O operation timeout
const CHUNK_DELAY: Duration = Duration::from_millis(100); // Delay between chunks
const MAX_CHUNK_SIZE: usize = 4194304; // Maximum chunk size (4 MiB)

#[test]
fn test_handle_put_rmbt() {
    // Setup logger
    let _ = env_logger::Builder::new()
        .filter_level(log::LevelFilter::Debug)
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

        // Read initial ACCEPT response
        let mut accept_response = [0u8; 1024];
        let n = timeout(IO_TIMEOUT, stream.read(&mut accept_response))
            .await
            .expect("Initial ACCEPT response timeout")
            .expect("Failed to read initial ACCEPT response");
        let accept_str = String::from_utf8_lossy(&accept_response[..n]);
        info!("Received initial ACCEPT response");
        assert!(accept_str.contains("ACCEPT"), "Server should send initial ACCEPT");

        // Test PUT command with increasing chunk sizes
        let mut current_chunks = 1;
        let mut current_chunk_size = CHUNK_SIZE;
        let mut rng = Rng::new();
        let start_time = Instant::now();

        while start_time.elapsed().as_secs() < TEST_DURATION {
            // Calculate next chunk size, respecting MAX_CHUNK_SIZE
            if current_chunks > MAX_CHUNKS {
                current_chunks = 1;
                let next_chunk_size = current_chunk_size * 2;
                current_chunk_size = if next_chunk_size <= MAX_CHUNK_SIZE {
                    next_chunk_size
                } else {
                    // Reset to initial size if we would exceed MAX_CHUNK_SIZE
                    CHUNK_SIZE
                };
            }

            // Send PUT command with current chunk size
            let command = format!("PUT {}\n", current_chunk_size);
            timeout(IO_TIMEOUT, stream.write_all(command.as_bytes()))
                .await
                .expect("Write command timeout")
                .expect("Failed to send PUT command");
            info!("Sent PUT command with chunk_size={}", current_chunk_size);

            // Read OK response
            let mut response = [0u8; 1024];
            let n = timeout(IO_TIMEOUT, stream.read(&mut response))
                .await
                .expect("OK response timeout")
                .expect("Failed to read OK response");
            let response_str = String::from_utf8_lossy(&response[..n]);
            info!("Received response: {}", response_str.trim());

            // Check if we got OK or TIME response
            if response_str.contains("OK") {
                info!("Got OK response");
            } else if response_str.contains("TIME") {
                info!("Got TIME response, waiting for OK");
                // Read next response which should be OK
                let n = timeout(IO_TIMEOUT, stream.read(&mut response))
                    .await
                    .expect("OK response timeout")
                    .expect("Failed to read OK response");
                let ok_response = String::from_utf8_lossy(&response[..n]);
                info!("Received next response: {}", ok_response.trim());
                assert!(ok_response.contains("OK"), "Server should respond with OK");
            } else {
                panic!("Server should respond with either OK or TIME");
            }

            // Send data chunks
            info!("Sending {} chunks of size {}", current_chunks, current_chunk_size);

            for i in 0..current_chunks {
                let mut chunk = vec![0u8; current_chunk_size];
                rng.fill(&mut chunk);

                // Set terminator byte
                let is_last_chunk = i == current_chunks - 1;
                chunk[current_chunk_size - 1] = if is_last_chunk { 0xFF } else { 0x00 };
                let terminator = chunk[current_chunk_size - 1];

                // Send chunk
                timeout(IO_TIMEOUT, stream.write_all(&chunk))
                    .await
                    .expect("Write chunk timeout")
                    .expect("Failed to send chunk");
                
                info!("Sent chunk {} of {} (terminator: {})", i + 1, current_chunks, terminator);

                // After sending the last chunk, wait for TIME response
                if is_last_chunk {
                    let mut attempts = 0;
                    let max_attempts = 5;
                    let mut got_time = false;

                    while attempts < max_attempts && !got_time {
                        match timeout(IO_TIMEOUT, stream.read(&mut response)).await {
                            Ok(Ok(n)) => {
                                let response_str = String::from_utf8_lossy(&response[..n]);
                                if response_str.contains("TIME") {
                                    info!("Received TIME response: {}", response_str.trim());
                                    // Verify TIME response format
                                    let parts: Vec<&str> = response_str.trim().split_whitespace().collect();
                                    assert!(parts.len() >= 4, "TIME response should have at least 4 parts");
                                    assert_eq!(parts[0], "TIME", "First part should be 'TIME'");
                                    assert!(parts[1].parse::<u64>().is_ok(), "Second part should be nanoseconds");
                                    assert_eq!(parts[2], "BYTES", "Third part should be 'BYTES'");
                                    assert!(parts[3].parse::<u64>().is_ok(), "Fourth part should be byte count");
                                    got_time = true;
                                }
                            }
                            _ => {
                                attempts += 1;
                                sleep(Duration::from_millis(100)).await;
                            }
                        }
                    }

                    assert!(got_time, "Failed to receive TIME response after final chunk");
                }

                // Add delay between chunks to simulate real client behavior
                if !is_last_chunk {
                    sleep(CHUNK_DELAY).await;
                }
            }

            // Increment chunks for next iteration
            current_chunks *= 2;
        }

        // Send QUIT command
        info!("Sending QUIT command");
        timeout(IO_TIMEOUT, stream.write_all(b"QUIT\n"))
            .await
            .expect("Write QUIT timeout")
            .expect("Failed to send QUIT");

        // Read ACCEPT response after QUIT
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

        info!("PUT test completed successfully");
    });
}

#[test]
fn test_handle_put_ws() {
    // Setup logger
    let _ = env_logger::Builder::new()
        .filter_level(log::LevelFilter::Debug)
        .format_timestamp_millis()
        .format_module_path(false)
        .format_target(false)
        .filter(Some("tokio"), log::LevelFilter::Info)  // Filter out WouldBlock messages
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

        // Read initial command list response
        let response = timeout(IO_TIMEOUT, ws_stream.next())
            .await
            .expect("Initial command list response timeout")
            .expect("Failed to read initial command list response")
            .expect("Failed to get message");
        let command_list = response.to_string();
        info!("Received initial command list: {}", command_list.trim());
        assert!(command_list.contains("ACCEPT"), "Server should send initial command list");

        // Test PUT command with increasing chunk sizes
        let mut current_chunks = 1;
        let mut current_chunk_size = CHUNK_SIZE;
        let mut rng = Rng::new();
        let start_time = Instant::now();

        while start_time.elapsed().as_secs() < TEST_DURATION {
            // Calculate next chunk size, respecting MAX_CHUNK_SIZE
            if current_chunks > MAX_CHUNKS {
                current_chunks = 1;
                let next_chunk_size = current_chunk_size * 2;
                current_chunk_size = if next_chunk_size <= MAX_CHUNK_SIZE {
                    next_chunk_size
                } else {
                    // Reset to initial size if we would exceed MAX_CHUNK_SIZE
                    CHUNK_SIZE
                };
            }

            // Send PUT command with current chunk size
            let command = format!("PUT {}\n", current_chunk_size);
            timeout(IO_TIMEOUT, ws_stream.send(Message::Text(command)))
                .await
                .expect("Write command timeout")
                .expect("Failed to send PUT command");
            info!("Sent PUT command with chunk_size={}", current_chunk_size);

            // Read OK response
            let response = timeout(IO_TIMEOUT, ws_stream.next())
                .await
                .expect("OK response timeout")
                .expect("Failed to read OK response")
                .expect("Failed to get message");
            let response_str = response.to_string();
            info!("Received response: {}", response_str.trim());

            // Check if we got OK or TIME response
            if response_str.contains("OK") {
                info!("Got OK response");
            } else if response_str.contains("TIME") {
                info!("Got TIME response, waiting for OK");
                // Read next response which should be OK
                let response = timeout(IO_TIMEOUT, ws_stream.next())
                    .await
                    .expect("OK response timeout")
                    .expect("Failed to read OK response")
                    .expect("Failed to get message");
                let ok_response = response.to_string();
                info!("Received next response: {}", ok_response.trim());
                assert!(ok_response.contains("OK"), "Server should respond with OK");
            } else {
                panic!("Server should respond with either OK or TIME");
            }

            // Send data chunks
            info!("Sending {} chunks of size {}", current_chunks, current_chunk_size);

            for i in 0..current_chunks {
                let mut chunk = vec![0u8; current_chunk_size];
                rng.fill(&mut chunk);

                // Set terminator byte
                let is_last_chunk = i == current_chunks - 1;
                chunk[current_chunk_size - 1] = if is_last_chunk { 0xFF } else { 0x00 };
                let terminator = chunk[current_chunk_size - 1];

                // Send chunk
                timeout(IO_TIMEOUT, ws_stream.send(Message::Binary(chunk)))
                    .await
                    .expect("Write chunk timeout")
                    .expect("Failed to send chunk");
                
                info!("Sent chunk {} of {} (terminator: {})", i + 1, current_chunks, terminator);

                // After sending the last chunk, wait for TIME response
                if is_last_chunk {
                    let mut attempts = 0;
                    let max_attempts = 5;
                    let mut got_time = false;

                    while attempts < max_attempts && !got_time {
                        match timeout(IO_TIMEOUT, ws_stream.next()).await {
                            Ok(Some(Ok(msg))) => {
                                let response_str = msg.to_string();
                                if response_str.contains("TIME") {
                                    info!("Received TIME response: {}", response_str.trim());
                                    // Verify TIME response format
                                    let parts: Vec<&str> = response_str.trim().split_whitespace().collect();
                                    assert!(parts.len() >= 4, "TIME response should have at least 4 parts");
                                    assert_eq!(parts[0], "TIME", "First part should be 'TIME'");
                                    assert!(parts[1].parse::<u64>().is_ok(), "Second part should be nanoseconds");
                                    assert_eq!(parts[2], "BYTES", "Third part should be 'BYTES'");
                                    assert!(parts[3].parse::<u64>().is_ok(), "Fourth part should be byte count");
                                    got_time = true;
                                }
                            }
                            _ => {
                                attempts += 1;
                                sleep(Duration::from_millis(100)).await;
                            }
                        }
                    }

                    assert!(got_time, "Failed to receive TIME response after final chunk");
                }

                // Add delay between chunks to simulate real client behavior
                if !is_last_chunk {
                    sleep(CHUNK_DELAY).await;
                }
            }

            // Increment chunks for next iteration
            current_chunks *= 2;
        }

        // Send QUIT command
        info!("Sending QUIT command");
        timeout(IO_TIMEOUT, ws_stream.send(Message::Text("QUIT\n".to_string())))
            .await
            .expect("Write QUIT timeout")
            .expect("Failed to send QUIT");

        // Read ACCEPT response after QUIT
        let response = timeout(IO_TIMEOUT, ws_stream.next())
            .await
            .expect("Final ACCEPT timeout")
            .expect("Failed to read final ACCEPT response")
            .expect("Failed to get message");
        let accept_response = response.to_string();
        info!("Received final ACCEPT response");
        assert!(accept_response.contains("ACCEPT"), "Server should respond with ACCEPT after QUIT");

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

        info!("WebSocket PUT test completed successfully");
    });
}
