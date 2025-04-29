#[path = "../test_utils/mod.rs"]
mod test_utils;

use tokio::runtime::Runtime;
use log::{info, debug, trace};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use crate::test_utils::TestServer;
use std::time::Duration;
use env_logger;
use std::collections::VecDeque;
use tokio::time::sleep;
use tokio::time::timeout;

const MIN_PINGS: u32 = 10;
const MAX_PINGS: u32 = 200;
const PING_DURATION: u64 = 2; // 2 seconds to ensure we can complete MIN_PINGS
const DELAY_MS: u64 = 50; // 50ms delay
const READ_TIMEOUT: Duration = Duration::from_millis(10); // 10ms timeout for reading

#[test]
fn test_handle_ping() {
    // Setup logger
    let _ = env_logger::Builder::new()
        .filter_level(log::LevelFilter::Trace)
        .format_timestamp_millis()
        .format_module_path(false)
        .format_target(false)
        .try_init();

    let rt = Runtime::new().unwrap();
    
    rt.block_on(async {
        info!("Starting PING test");
        
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
        
        // Run test for 1 second
        let start_time = std::time::Instant::now();
        let test_duration = Duration::from_secs(PING_DURATION);
        let mut total_pings = 0;
        let mut server_times = VecDeque::new();
        
        info!("Starting PING loop for {} seconds", PING_DURATION);
        
        while start_time.elapsed() < test_duration && total_pings < MAX_PINGS {
            trace!("PING iteration #{}", total_pings + 1);
            
            // Send PING command
            debug!("Sending PING command #{}", total_pings + 1);
            stream.write_all(b"PING\n").await.expect("Failed to send PING");
            trace!("PING command sent");
            
            // Read PONG response
            let mut response = [0u8; 1024];
            let n = stream.read(&mut response).await.expect("Failed to read PONG response");
            let pong_response = String::from_utf8_lossy(&response[..n]);
            debug!("Received PONG response (raw bytes): {:?}", &response[..n]);
            debug!("Received PONG response: {}", pong_response);
            
            // Send OK immediately after receiving PONG
            debug!("Sending OK response");
            stream.write_all(b"OK\n").await.expect("Failed to send OK");
            stream.flush().await.expect("Failed to flush OK response");
            trace!("OK response sent");
            
            if pong_response.contains("ERR") {
                debug!("Received {} response, stopping PING test", pong_response);
                break;
            }
            
            assert!(pong_response.contains("PONG"), "Server should respond with PONG");
            trace!("PONG response received");
            
            // Read TIME response
            let mut time_response = [0u8; 1024];
            let n = stream.read(&mut time_response).await.expect("Failed to read TIME response");
            let time_str = String::from_utf8_lossy(&time_response[..n]);
            debug!("Received TIME response (raw bytes): {:?}", &time_response[..n]);
            debug!("Received TIME response: {}", time_str);
            trace!("TIME response received");
            
            // Parse time
            let time_ns = time_str.trim()
                .split_whitespace()
                .nth(1)
                .expect("No time value in response")
                .parse::<u64>()
                .expect("Failed to parse time");
            
            assert!(time_ns > 0, "Time should be positive");
            server_times.push_back(time_ns);
            trace!("Time parsed: {} ns", time_ns);
            
            // Read ACCEPT_GETCHUNKS response
            let mut accept_response = [0u8; 1024];
            let n = stream.read(&mut accept_response).await.expect("Failed to read ACCEPT_GETCHUNKS response");
            let accept_str = String::from_utf8_lossy(&accept_response[..n]);
            debug!("Received ACCEPT_GETCHUNKS response: {}", accept_str);
            assert!(accept_str.contains("ACCEPT"), "Server should send ACCEPT_GETCHUNKS message");
            
            total_pings += 1;
            trace!("PING iteration #{} completed", total_pings);
        }
        
        info!("PING loop completed after {} iterations", total_pings);
        
        // Verify minimum number of pings
        assert!(total_pings >= MIN_PINGS, "Should receive at least {} PING responses", MIN_PINGS);
        assert!(total_pings <= MAX_PINGS, "Should not receive more than {} PING responses", MAX_PINGS);
        
        // Sort server times to find median
        let mut sorted_times: Vec<u64> = server_times.into_iter().collect();
        sorted_times.sort();
        
        let median_time = if sorted_times.len() % 2 == 0 {
            let mid = sorted_times.len() / 2;
            (sorted_times[mid - 1] + sorted_times[mid]) / 2
        } else {
            sorted_times[sorted_times.len() / 2]
        };
        
        info!("Test completed: {} pings, median server time: {} ns", 
               total_pings, median_time);
    });
}

#[test]
fn test_handle_ping_ws() {
    // Setup logger
    let _ = env_logger::Builder::new()
        .filter_level(log::LevelFilter::Trace)
        .format_timestamp_millis()
        .format_module_path(false)
        .format_target(false)
        .try_init();

    let rt = Runtime::new().unwrap();
    
    rt.block_on(async {
        info!("Starting WebSocket PING test");
        
        // Create test server with WebSocket enabled
        let server = TestServer::new(Some(8080), None).unwrap();
        info!("Test server created with WebSocket enabled");
        
        // Connect to server using WebSocket
        let (mut stream, _) = server.connect_rmbtd().await.expect("Failed to connect to server");
        info!("Connected to server via WebSocket");
        
        // Read initial ACCEPT message after token validation
        let mut initial_accept = [0u8; 1024];
        let n = stream.read(&mut initial_accept).await.expect("Failed to read initial ACCEPT");
        let accept_str = String::from_utf8_lossy(&initial_accept[..n]);
        info!("Initial ACCEPT message: {}", accept_str);
        assert!(accept_str.contains("ACCEPT"), "Server should send initial ACCEPT message");
        
        // Run test for 2 seconds
        let start_time = std::time::Instant::now();
        let test_duration = Duration::from_secs(PING_DURATION);
        let mut total_pings = 0;
        let mut server_times = VecDeque::new();
        
        info!("Starting WebSocket PING loop for {} seconds", PING_DURATION);
        
        while start_time.elapsed() < test_duration && total_pings < MAX_PINGS {
            trace!("WebSocket PING iteration #{}", total_pings + 1);
            
            // Send PING command
            debug!("Sending WebSocket PING command #{}", total_pings + 1);
            stream.write_all(b"PING\n").await.expect("Failed to send PING");
            trace!("WebSocket PING command sent");
            
            // Read PONG response
            let mut response = [0u8; 1024];
            let n = stream.read(&mut response).await.expect("Failed to read PONG response");
            let pong_response = String::from_utf8_lossy(&response[..n]);
            debug!("Received WebSocket PONG response (raw bytes): {:?}", &response[..n]);
            debug!("Received WebSocket PONG response: {}", pong_response);
            
            // Send OK immediately after receiving PONG
            debug!("Sending WebSocket OK response");
            stream.write_all(b"OK\n").await.expect("Failed to send OK");
            stream.flush().await.expect("Failed to flush OK response");
            trace!("WebSocket OK response sent");
            
            if pong_response.contains("ERR") {
                debug!("Received {} response, stopping WebSocket PING test", pong_response);
                break;
            }
            
            assert!(pong_response.contains("PONG"), "Server should respond with PONG");
            trace!("WebSocket PONG response received");
            
            // Read TIME response
            let mut time_response = [0u8; 1024];
            let n = stream.read(&mut time_response).await.expect("Failed to read TIME response");
            let time_str = String::from_utf8_lossy(&time_response[..n]);
            debug!("Received WebSocket TIME response (raw bytes): {:?}", &time_response[..n]);
            debug!("Received WebSocket TIME response: {}", time_str);
            trace!("WebSocket TIME response received");
            
            // Parse time
            let time_ns = time_str.trim()
                .split_whitespace()
                .nth(1)
                .expect("No time value in response")
                .parse::<u64>()
                .expect("Failed to parse time");
            
            assert!(time_ns > 0, "Time should be positive");
            server_times.push_back(time_ns);
            trace!("WebSocket time parsed: {} ns", time_ns);
            
            // Read ACCEPT_GETCHUNKS response
            let mut accept_response = [0u8; 1024];
            let n = stream.read(&mut accept_response).await.expect("Failed to read ACCEPT_GETCHUNKS response");
            let accept_str = String::from_utf8_lossy(&accept_response[..n]);
            debug!("Received WebSocket ACCEPT_GETCHUNKS response: {}", accept_str);
            assert!(accept_str.contains("ACCEPT"), "Server should send ACCEPT_GETCHUNKS message");
            
            total_pings += 1;
            trace!("WebSocket PING iteration #{} completed", total_pings);
        }
        
        info!("WebSocket PING loop completed after {} iterations", total_pings);
        
        // Verify minimum number of pings
        assert!(total_pings >= MIN_PINGS, "Should receive at least {} WebSocket PING responses", MIN_PINGS);
        assert!(total_pings <= MAX_PINGS, "Should not receive more than {} WebSocket PING responses", MAX_PINGS);
        
        // Sort server times to find median
        let mut sorted_times: Vec<u64> = server_times.into_iter().collect();
        sorted_times.sort();
        
        let median_time = if sorted_times.len() % 2 == 0 {
            let mid = sorted_times.len() / 2;
            (sorted_times[mid - 1] + sorted_times[mid]) / 2
        } else {
            sorted_times[sorted_times.len() / 2]
        };
        
        info!("WebSocket test completed: {} pings, median server time: {} ns", 
               total_pings, median_time);
    });
} 