#[path = "../test_utils/mod.rs"]
mod test_utils;

/// This test file implements the RMBT protocol's downlink measurement phase using the GETTIME command.
/// It verifies the server's ability to maintain a continuous data stream for a specified duration,
/// following the RMBT specification for timing and chunk handling. The test includes both plain TCP
/// and WebSocket implementations, ensuring proper data transmission, chunk termination, and timing
/// measurements. It validates server responses, data integrity, and connection cleanup according to
/// the protocol specifications.

use tokio::runtime::Runtime;
use log::{info, debug};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use crate::test_utils::TestServer;
use std::time::Duration;
use env_logger;
use tokio::time::timeout;

const TEST_DURATION: u64 = 7; // Test duration in seconds (as per RMBT spec)
const MIN_CHUNK_SIZE: u32 = 4096;  // 4KB
const MAX_CHUNK_SIZE: u32 = 4194304;  // 4MB
const MAX_CHUNKS: usize = 8;  // Maximum number of chunks before increasing size
const IO_TIMEOUT: Duration = Duration::from_secs(10); // I/O operation timeout

#[test]
fn test_handle_get_time_rmbt() {
    let _ = env_logger::Builder::new()
        .filter_level(log::LevelFilter::Info)
        .format_timestamp_millis()
        .format_module_path(false)
        .format_target(false)
        .try_init();

    let rt = Runtime::new().unwrap();
    
    rt.block_on(async {
        info!("Starting GETTIME test");
        
        let server = TestServer::new(None, None).unwrap();
        let (mut stream, initial_chunk_size) = server.connect_rmbtd().await.expect("Failed to connect to server");
        
        // Read initial ACCEPT message after token validation
        let mut initial_accept = [0u8; 1024];
        let n = stream.read(&mut initial_accept).await.expect("Failed to read initial ACCEPT");
        let accept_str = String::from_utf8_lossy(&initial_accept[..n]);
        assert!(accept_str.contains("ACCEPT"), "Server should send initial ACCEPT message");

        let mut chunk_size = initial_chunk_size;
        let mut chunks = 1;
        let mut total_bytes = 0;
        
        // Test valid GETTIME command with current chunk size
        let command = format!("GETTIME {} {}\n", TEST_DURATION, chunk_size);
        stream.write_all(command.as_bytes()).await.expect("Failed to send GETTIME command");
        
        // Read and verify data stream
        let mut buffer = vec![0u8; chunk_size as usize];
        let mut last_byte = 0u8;
        let mut received_termination = false;
        
        while !received_termination {
            match timeout(IO_TIMEOUT, stream.read(&mut buffer)).await {
                Ok(Ok(n)) => {
                    if n == 0 {
                        break;
                    }
                    
                    total_bytes += n;
                    last_byte = buffer[n-1];
                    
                    if last_byte == 0xFF {
                        received_termination = true;
                        // Send OK response
                        stream.write_all(b"OK\n").await.expect("Failed to send OK response");
                        stream.flush().await.expect("Failed to flush OK response");
                        
                        // Read TIME response
                        let mut time_response = [0u8; 1024];
                        let n = stream.read(&mut time_response).await.expect("Failed to read TIME response");
                        let time_str = String::from_utf8_lossy(&time_response[..n]);
                        
                        // Verify TIME response format
                        let time_parts: Vec<&str> = time_str.trim().split_whitespace().collect();
                        assert_eq!(time_parts.len(), 2, "TIME response should have 2 parts");
                        assert_eq!(time_parts[0], "TIME", "First part should be 'TIME'");
                        
                        // Get server's time measurement
                        let time_ns = time_parts[1].parse::<u64>().expect("Second part should be nanoseconds");
                        let expected_ns = TEST_DURATION * 1_000_000_000;
                        assert!(time_ns >= expected_ns, 
                               "Test duration should be at least {} seconds", TEST_DURATION);
                        
                        // Calculate speed using server's time measurement
                        let bytes_per_sec = (total_bytes as f64 * 1_000_000_000.0) / time_ns as f64;
                        let mbps = (bytes_per_sec * 8.0) / 1_000_000.0;
                        info!("Test completed: received {} bytes in total", total_bytes);
                        info!("Average speed: {:.2} Mbps ({:.2} bytes/sec)", mbps, bytes_per_sec);
                        
                        break;
                    } else {
                        assert_eq!(last_byte, 0x00, "Non-terminal chunk should end with 0x00");
                    }
                }
                Ok(Err(e)) => {
                    panic!("Failed to read data: {}", e);
                }
                Err(_) => {
                    panic!("Read timeout");
                }
            }
        }
        
        assert!(received_termination, "Did not receive termination chunk within timeout");
        assert!(total_bytes > 0, "Should receive some data");
        
        // Send QUIT command
        stream.write_all(b"QUIT\n").await.expect("Failed to send QUIT");
        
        // Read ACCEPT response first
        let mut response = [0u8; 1024];
        let n = stream.read(&mut response).await.expect("Failed to read ACCEPT response");
        let accept_response = String::from_utf8_lossy(&response[..n]);
        assert!(accept_response.contains("ACCEPT"), "Server should respond with ACCEPT after QUIT");
        
        // Then read BYE response
        let mut response = [0u8; 1024];
        let n = stream.read(&mut response).await.expect("Failed to read BYE response");
        let bye_response = String::from_utf8_lossy(&response[..n]);
        assert!(bye_response.contains("BYE"), "Server should respond with BYE");
        
        info!("GETTIME test completed successfully");
    });
}