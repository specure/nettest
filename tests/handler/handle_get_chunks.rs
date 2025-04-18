#[path = "../test_utils/mod.rs"]
mod test_utils;

use tokio::runtime::Runtime;
use log::{info, debug, trace};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use crate::test_utils::{find_free_port, TestServer};

#[test]
fn test_handle_get_chunks() {
    // Используем хардкодный ключ
    let key = "q4aFShYnBgoYyDr4cxes0DSYCvjLpeKJjhCfvmVCdiIpsdeU1djvBtE6CMtNCbDWkiU68X7bajIAwLon14Hh7Wpi5MJWJL7HXokh";
    
    // Находим два свободных порта
    let port1 = find_free_port();
    let port2 = find_free_port();
    
    info!("Using ports: {} and {}", port1, port2);
    
    // Запускаем сервер
    let server = TestServer::new(port1, port2);
    
    // Создаем runtime для асинхронных операций
    let rt = Runtime::new().unwrap();
    
    // Пробуем подключиться к серверу и проверить протокол
    rt.block_on(async {
        let mut stream = server.connect(false).await.expect("Failed to connect to server");
        
        // Читаем приветствие сервера
        let mut buf = [0u8; 1024];
        let n = stream.read(&mut buf).await.expect("Failed to read version");
        let version = String::from_utf8_lossy(&buf[..n]);
        debug!("Received version: {}", version);
        assert!(version.contains("RMBTv"), "Server greeting should contain version");
        assert!(version.contains("ACCEPT"), "Server greeting should contain ACCEPT");
        assert!(version.contains("TOKEN"), "Server greeting should contain TOKEN");
        assert!(version.contains("QUIT"), "Server greeting should contain QUIT");

        // Отправляем токен
        TestServer::send_token(&mut stream, key).await.expect("Failed to send token");
        
        // Отправляем GETCHUNKS команду
        let chunks = 3; // Количество чанков
        let chunk_size = 4096; // 4KB
        debug!("Sending GETCHUNKS command: chunks={}, chunk_size={}", chunks, chunk_size);
        let getchunks_cmd = format!("GETCHUNKS {} {}\n", chunks, chunk_size);
        debug!("GETCHUNKS command bytes: {:?}", getchunks_cmd.as_bytes());
        stream.write_all(getchunks_cmd.as_bytes())
            .await
            .expect("Failed to send GETCHUNKS");
        stream.flush().await.expect("Failed to flush GETCHUNKS command");
        debug!("GETCHUNKS command sent and flushed");
            
        // Читаем данные
        let mut total_bytes = 0;
        let mut chunks_received = 0;
        let mut found_terminator = false;
        let mut buf = vec![0u8; chunk_size * 2]; // Double buffer size to handle larger chunks
        
        // Читаем данные до тех пор, пока не найдем терминатор
        println!("Waiting to read data chunks...");
        while chunks_received < chunks {
            match stream.read(&mut buf).await {
                Ok(n) => {
                    if n == 0 {
                        debug!("Connection closed by server during data transfer");
                        break;
                    }
                    
                    total_bytes += n;
                    chunks_received += 1;
                    trace!("Received chunk {}/{}: {} bytes, total: {}", 
                           chunks_received, chunks, n, total_bytes);
                    
                    // Подробное логирование содержимого чанка
                    trace!("Chunk data (first 32 bytes): {:?}", &buf[..std::cmp::min(32, n)]);
                    trace!("Chunk data (last 32 bytes): {:?}", &buf[n.saturating_sub(32)..n]);
                    trace!("Last byte: 0x{:02X}", buf[n - 1]);
                    
                    // Проверяем последний байт
                    if n > 0 {
                        if chunks_received == chunks {
                            assert_eq!(buf[n - 1], 0xFF, "Last chunk should have termination byte 0xFF");
                            found_terminator = true;
                        } else {
                            assert_eq!(buf[n - 1], 0x00, "Non-last chunk should have byte 0x00");
                        }
                    }
                }
                Err(e) => {
                    panic!("Failed to read data: {}", e);
                }
            }
        }
        
        // Проверяем, что получили все чанки и терминатор
        assert_eq!(chunks_received, chunks, "Should receive exactly {} chunks", chunks);
        assert!(found_terminator, "Should receive termination byte");
        
        // Отправляем OK после получения всех данных
        debug!("Sending OK response to server");
        stream.write_all(b"OK\n").await.expect("Failed to send OK response");
        stream.flush().await.expect("Failed to flush OK response");
        debug!("OK response sent and flushed");

        // Читаем TIME ответ с таймаутом
        debug!("Reading TIME response from server");
        let mut response = [0u8; 1024];
        let mut time_response = String::new();
        let mut attempts = 0;
        const MAX_ATTEMPTS: u32 = 5;
        
        // Читаем ответ до символа новой строки или таймаута
        while attempts < MAX_ATTEMPTS {
            match tokio::time::timeout(
                std::time::Duration::from_secs(1),
                stream.read(&mut response)
            ).await {
                Ok(Ok(n)) => {
                    if n == 0 {
                        debug!("Connection closed by server while reading TIME response");
                        break;
                    }
                    let chunk = String::from_utf8_lossy(&response[..n]);
                    trace!("Received chunk: '{}'", chunk);
                    time_response.push_str(&chunk);
                    if time_response.contains('\n') {
                        break;
                    }
                }
                Ok(Err(e)) => {
                    panic!("Failed to read TIME response: {}", e);
                }
                Err(_) => {
                    debug!("Timeout while reading TIME response, attempt {}/{}", attempts + 1, MAX_ATTEMPTS);
                    attempts += 1;
                    continue;
                }
            }
        }
        
        let time_response = time_response.trim();
        debug!("Final TIME response: '{}'", time_response);
        assert!(
            time_response.starts_with("TIME "), 
            "Server should respond with TIME, got: '{}'", 
            time_response
        );
        
        // Проверяем, что получили данные
        assert!(total_bytes > 0, "Should receive some data");
        assert_eq!(total_bytes, chunks * chunk_size, 
                  "Should receive exactly {} chunks of {} bytes each", 
                  chunks, chunk_size);
        
        // Отправляем QUIT
        debug!("Sending QUIT command");
        TestServer::send_quit(&mut stream).await.expect("Failed to send QUIT");
        
        // Закрываем соединение
        stream.shutdown().await.expect("Failed to shutdown stream");
    });
} 