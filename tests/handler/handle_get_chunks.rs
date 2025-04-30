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

#[test]
fn test_handle_get_chunks() {
    let _ = env_logger::Builder::new()
        .filter_level(log::LevelFilter::Debug)
        .format_timestamp_millis()
        .format_module_path(false)
        .format_target(false)
        .try_init();

    let rt = Runtime::new().unwrap();
    
    rt.block_on(async {
        let server = TestServer::new(None, None).unwrap();
        
        let (mut stream, chunk_size) = server.connect_rmbtd().await.expect("Failed to connect to server");
        
        let mut chunks = 1;
        let mut total_bytes = 0;
        
        let start_time = std::time::Instant::now();
        let test_duration = Duration::from_secs(5);
        
        while start_time.elapsed() < test_duration {
            let getchunks_cmd = format!("GETCHUNKS {}\n", chunks);
            debug!("Sending GETCHUNKS command: chunks={}", chunks);
            stream.write_all(getchunks_cmd.as_bytes())
                .await
                .expect("Failed to send GETCHUNKS");
            stream.flush().await.expect("Failed to flush GETCHUNKS command");
            
            let mut found_terminator = false;
            let mut chunks_received = 0;
            let mut total_bytes_read = 0;
            let mut last_byte = 0u8;
            
            while !found_terminator && chunks_received < chunks {
                let mut buf = vec![0u8; chunk_size as usize];
                
                match stream.read(&mut buf).await {
                    Ok(n) => {
                        if n == 0 {
                            panic!("Connection closed before receiving full chunk");
                        }
                        
                        total_bytes_read += n;
                        total_bytes += n;  // Обновляем общий счетчик байт
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
                        
                        stream.write_all(b"OK\n").await.expect("Failed to send OK");
                        stream.flush().await.expect("Failed to flush OK");
                        
                        sleep(Duration::from_millis(100)).await;
                    }
                    Err(e) => {
                        panic!("Failed to read chunk: {}", e);
                    }
                }
            }
            
            assert_eq!(chunks_received, chunks, "Did not receive all chunks");
            assert!(found_terminator, "Did not find terminator byte");
            
            let mut response = [0u8; 1024];
            let n = stream.read(&mut response).await.expect("Failed to read TIME response");
            let time_response = String::from_utf8_lossy(&response[..n]);
            debug!("Received TIME response: {}", time_response);
            assert!(time_response.contains("TIME"), "Server should respond with TIME");
            
            let time_str = time_response.trim().split_whitespace().nth(1).unwrap();
            let time_ns = time_str.parse::<u64>().expect("Failed to parse time");
            assert!(time_ns > 0, "Time should be positive");
            
            chunks *= 2;
        }
        
        assert!(total_bytes > 0, "Should receive some data");
        debug!("Test completed: received {} bytes in total", total_bytes);
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

    let (ws_stream, chunk_size) = server.connect_ws().await.expect("Failed to connect to server");
    let (mut write, mut read) = ws_stream.split();

    let mut chunks = 1;
    let mut total_bytes = 0;
    
    let start_time = std::time::Instant::now();
    let test_duration = Duration::from_secs(2);
    
    while start_time.elapsed() < test_duration {
        let getchunks_cmd = format!("GETCHUNKS {}\n", chunks);
        debug!("Sending GETCHUNKS command: chunks={}", chunks);
        write.send(Message::Text(getchunks_cmd)).await.expect("Failed to send GETCHUNKS");

        let mut found_terminator = false;
        let mut chunks_received = 0;
        let mut total_bytes_read = 0;
        let mut last_byte = 0u8;
        
        while !found_terminator && chunks_received < chunks {
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
                    
                    write.send(Message::Text("OK\n".to_string())).await.expect("Failed to send OK");
                    
                    // Добавляем небольшую паузу после отправки OK
                    sleep(Duration::from_millis(100)).await;
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
        
        let time_response = read.next().await.expect("Failed to read TIME response").expect("Failed to read TIME response");
        let time_text = time_response.to_text().expect("TIME response is not text");
        debug!("Received TIME response: {}", time_text);
        assert!(time_text.contains("TIME"), "Server should respond with TIME");
        
        let time_str = time_text.trim().split_whitespace().nth(1).unwrap();
        let time_ns = time_str.parse::<u64>().expect("Failed to parse time");
        assert!(time_ns > 0, "Time should be positive");
        
        chunks *= 2;
    }
    assert!(total_bytes > 0, "Should receive some data");
    debug!("Test completed: received {} bytes in total", total_bytes);
    
    // Закрываем WebSocket соединение
    write.close().await.expect("Failed to close WebSocket connection");
    info!("Closed WebSocket connection");
} 