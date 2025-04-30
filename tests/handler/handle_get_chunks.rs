#[path = "../test_utils/mod.rs"]
mod test_utils;

use tokio::runtime::Runtime;
use log::{info, debug, trace};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use crate::test_utils::{find_free_port, TestServer};
use std::time::Duration;
use std::env;
use env_logger;
use tokio::time::sleep;
use tokio_tungstenite::tungstenite::{Message, handshake::client::Request};
use futures_util::{SinkExt, StreamExt};
use tokio_native_tls::TlsConnector;
use native_tls::TlsConnector as NativeTlsConnector;
use tokio::net::TcpStream;
use uuid::Uuid;

const DEFAULT_PLAIN_PORT: u16 = 8080;
const DEFAULT_TLS_PORT: u16 = 443;

#[test]
fn test_handle_get_chunks() {
    // Настраиваем логгер
    let _ = env_logger::Builder::new()
        .filter_level(log::LevelFilter::Debug)
        .format_timestamp_millis()
        .format_module_path(false)
        .format_target(false)
        .try_init();

    let rt = Runtime::new().unwrap();
    
    rt.block_on(async {
        // Создаем тестовый сервер (он может быть dummy если используем дефолтные порты)
        let server = TestServer::new(None, None).unwrap();
        
        // Подключаемся к серверу и получаем размер чанка
        let (mut stream, chunk_size) = server.connect_rmbtd().await.expect("Failed to connect to server");
        
        // Начинаем с 1 чанка и удваиваем на каждой итерации
        let mut chunks = 1;
        let mut total_bytes = 0;
        
        // Выполняем тест в течение 2 секунд (d из спецификации)
        let start_time = std::time::Instant::now();
        let test_duration = Duration::from_secs(5);
        
        while start_time.elapsed() < test_duration {
            // Отправляем GETCHUNKS команду
            let getchunks_cmd = format!("GETCHUNKS {}\n", chunks);
            debug!("Sending GETCHUNKS command: chunks={}", chunks);
            stream.write_all(getchunks_cmd.as_bytes())
                .await
                .expect("Failed to send GETCHUNKS");
            stream.flush().await.expect("Failed to flush GETCHUNKS command");
            
            // Читаем чанки
            let mut found_terminator = false;
            let mut chunks_received = 0;
            let mut total_bytes_read = 0;
            let mut last_byte = 0u8;
            
            while !found_terminator && chunks_received < chunks {
                // Создаем буфер для чтения данных
                let mut buf = vec![0u8; chunk_size as usize];
                
                // Читаем данные
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
                        
                        // Проверяем последний байт
                        if last_byte == 0xFF {
                            found_terminator = true;
                            chunks_received += 1;
                            debug!("Found terminator byte 0xFF, received chunk {}/{}", chunks_received, chunks);
                        } else if last_byte == 0x00 {
                            chunks_received += 1;
                            debug!("Received chunk {}/{}", chunks_received, chunks);
                        }
                        
                        // Отправляем OK после каждого чанка
                        stream.write_all(b"OK\n").await.expect("Failed to send OK");
                        stream.flush().await.expect("Failed to flush OK");
                        
                        // Добавляем небольшую паузу после отправки OK
                        sleep(Duration::from_millis(100)).await;
                    }
                    Err(e) => {
                        panic!("Failed to read chunk: {}", e);
                    }
                }
            }
            
            // Проверяем, что получили все чанки
            assert_eq!(chunks_received, chunks, "Did not receive all chunks");
            assert!(found_terminator, "Did not find terminator byte");
            
            // Читаем TIME ответ
            let mut response = [0u8; 1024];
            let n = stream.read(&mut response).await.expect("Failed to read TIME response");
            let time_response = String::from_utf8_lossy(&response[..n]);
            debug!("Received TIME response: {}", time_response);
            assert!(time_response.contains("TIME"), "Server should respond with TIME");
            
            // Проверяем, что время в наносекундах
            let time_str = time_response.trim().split_whitespace().nth(1).unwrap();
            let time_ns = time_str.parse::<u64>().expect("Failed to parse time");
            assert!(time_ns > 0, "Time should be positive");
            
            // Удваиваем количество чанков для следующей итерации
            chunks *= 2;
        }
        
        // Проверяем, что получили данные
        assert!(total_bytes > 0, "Should receive some data");
        debug!("Test completed: received {} bytes in total", total_bytes);
    });
}

async fn connect_ws(server: &TestServer) -> Result<(tokio_tungstenite::WebSocketStream<tokio_native_tls::TlsStream<TcpStream>>, u32), Box<dyn std::error::Error>> {
    // Create TLS connector with custom configuration
    let mut tls_connector = NativeTlsConnector::builder();
    tls_connector.danger_accept_invalid_certs(true);
    tls_connector.danger_accept_invalid_hostnames(true);
    let tls_connector = TlsConnector::from(tls_connector.build().unwrap());

    // Connect to the TLS port first
    info!("Connecting to TLS port {}", server.tls_port());
    let tcp_stream = TcpStream::connect(format!("{}:{}", server.host(), server.tls_port())).await.unwrap();
    
    // Determine host based on environment variable
    let host = if env::var("TEST_USE_DEFAULT_PORTS").is_ok() {
        "dev.measurementservers.net"
    } else {
        "localhost"
    };
    
    let tls_stream = tls_connector.connect(host, tcp_stream).await.unwrap();

    // Create WebSocket request
    let request = Request::builder()
        .uri(format!("wss://{}:{}/rmbt", host, server.tls_port()))
        .header("Host", format!("{}:{}", host, server.tls_port()))
        .header("Connection", "Upgrade")
        .header("Upgrade", "websocket")
        .header("Sec-WebSocket-Version", "13")
        .header("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ==")
        .body(())
        .unwrap();

    // Upgrade TLS connection to WebSocket
    let (ws_stream, _) = tokio_tungstenite::client_async(request, tls_stream).await.expect("Failed to create WebSocket");
    info!("WebSocket connection established");

    // Split the WebSocket stream into sender and receiver
    let (mut write, mut read) = ws_stream.split();

    // Read greeting message
    let greeting = read.next().await.expect("Failed to read greeting").expect("Failed to read greeting");
    let greeting_text = greeting.to_text().expect("Greeting is not text");
    info!("Received greeting: {}", greeting_text);
    assert!(greeting_text.contains("RMBTv"), "Server should send version in greeting");

    // Read ACCEPT TOKEN message
    let accept_token = read.next().await.expect("Failed to read ACCEPT TOKEN").expect("Failed to read ACCEPT TOKEN");
    let accept_token_text = accept_token.to_text().expect("ACCEPT TOKEN is not text");
    info!("Received ACCEPT TOKEN message: {}", accept_token_text);
    assert!(accept_token_text.contains("ACCEPT TOKEN"), "Server should send ACCEPT TOKEN message");

    // Generate and send token
    let token = format!("TOKEN {}", Uuid::new_v4());
    info!("Sending token: {}", token);
    write.send(Message::Text(token)).await.expect("Failed to send token");

    // Read token response
    let response = read.next().await.expect("Failed to read token response").expect("Failed to read token response");
    let response_text = response.to_text().expect("Response is not text");
    info!("Received token response: {}", response_text);
    assert!(response_text.contains("OK"), "Server should accept valid token");

    // Read CHUNKSIZE message
    let chunksize_response = read.next().await.expect("Failed to read CHUNKSIZE").expect("Failed to read CHUNKSIZE");
    let chunksize_text = chunksize_response.to_text().expect("CHUNKSIZE is not text");
    info!("Received CHUNKSIZE message: {}", chunksize_text);
    assert!(chunksize_text.contains("CHUNKSIZE"), "Server should send CHUNKSIZE message");

    // Parse chunk size from message
    let chunk_size = chunksize_text.split_whitespace()
        .nth(1)
        .expect("No chunk size in message")
        .parse::<u32>()
        .expect("Failed to parse chunk size");

    // Recombine the stream
    let ws_stream = write.reunite(read).expect("Failed to reunite WebSocket stream");

    Ok((ws_stream, chunk_size))
}

#[tokio::test]
async fn test_handle_get_chunks_ws() {
    // Настраиваем логгер
    let _ = env_logger::Builder::new()
        .filter_level(log::LevelFilter::Debug)
        .format_timestamp_millis()
        .format_module_path(false)
        .format_target(false)
        .try_init();

    // Создаем тестовый сервер
    let server = TestServer::new(None, None).unwrap();

    // Подключаемся к серверу и получаем размер чанка
    let (ws_stream, chunk_size) = server.connect_ws().await.expect("Failed to connect to server");
    let (mut write, mut read) = ws_stream.split();

    // Начинаем с 1 чанка и удваиваем на каждой итерации
    let mut chunks = 1;
    let mut total_bytes = 0;
    
    // Выполняем тест в течение 2 секунд
    let start_time = std::time::Instant::now();
    let test_duration = Duration::from_secs(2);
    
    while start_time.elapsed() < test_duration {
        // Отправляем GETCHUNKS команду
        let getchunks_cmd = format!("GETCHUNKS {}\n", chunks);
        debug!("Sending GETCHUNKS command: chunks={}", chunks);
        write.send(Message::Text(getchunks_cmd)).await.expect("Failed to send GETCHUNKS");

        // Читаем чанки
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
                    
                    // Проверяем последний байт
                    if last_byte == 0xFF {
                        found_terminator = true;
                        chunks_received += 1;
                        debug!("Found terminator byte 0xFF, received chunk {}/{}", chunks_received, chunks);
                    } else if last_byte == 0x00 {
                        chunks_received += 1;
                        debug!("Received chunk {}/{}", chunks_received, chunks);
                    }
                    
                    // Отправляем OK после каждого чанка
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
        
        // Проверяем, что получили все чанки
        assert_eq!(chunks_received, chunks, "Did not receive all chunks");
        assert!(found_terminator, "Did not find terminator byte");
        
        // Читаем TIME ответ
        let time_response = read.next().await.expect("Failed to read TIME response").expect("Failed to read TIME response");
        let time_text = time_response.to_text().expect("TIME response is not text");
        debug!("Received TIME response: {}", time_text);
        assert!(time_text.contains("TIME"), "Server should respond with TIME");
        
        // Проверяем, что время в наносекундах
        let time_str = time_text.trim().split_whitespace().nth(1).unwrap();
        let time_ns = time_str.parse::<u64>().expect("Failed to parse time");
        assert!(time_ns > 0, "Time should be positive");
        
        // Удваиваем количество чанков для следующей итерации
        chunks *= 2;
    }
    
    // Проверяем, что получили данные
    assert!(total_bytes > 0, "Should receive some data");
    debug!("Test completed: received {} bytes in total", total_bytes);
    
    // Закрываем WebSocket соединение
    write.close().await.expect("Failed to close WebSocket connection");
    info!("Closed WebSocket connection");
} 