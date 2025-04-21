#[path = "../test_utils/mod.rs"]
mod test_utils;

use tokio::runtime::Runtime;
use log::{info, debug, trace};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use crate::test_utils::{find_free_port, TestServer};
use rand::RngCore;

#[test]
fn test_handle_put_no_result() {
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
        
        // Отправляем HTTP запрос с Upgrade: rmbt
        let http_request = "GET / HTTP/1.1\r\nHost: localhost\r\nUpgrade: rmbt\r\nConnection: Upgrade\r\n\r\n";
        stream.write_all(http_request.as_bytes()).await.expect("Failed to send HTTP request");
        stream.flush().await.expect("Failed to flush HTTP request");
        
        // Читаем ответ на HTTP запрос
        let mut buf = [0u8; 1024];
        let n = stream.read(&mut buf).await.expect("Failed to read HTTP response");
        let response = String::from_utf8_lossy(&buf[..n]);
        debug!("Received HTTP response: {}", response);
        assert!(response.contains("101 Switching Protocols"), "Server should upgrade to RMBT");
        assert!(response.contains("Upgrade: rmbt"), "Server should upgrade to RMBT");
        
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
        
        // Отправляем PUTNORESULT команду
        let chunk_size = 4096; // 4KB
        debug!("Sending PUTNORESULT command: chunk_size={}", chunk_size);
        let put_cmd = format!("PUTNORESULT {}\n", chunk_size);
        debug!("PUTNORESULT command bytes: {:?}", put_cmd.as_bytes());
        stream.write_all(put_cmd.as_bytes())
            .await
            .expect("Failed to send PUTNORESULT");
        stream.flush().await.expect("Failed to flush PUTNORESULT command");
        debug!("PUTNORESULT command sent and flushed");
        
        // Читаем OK ответ от сервера
        let mut response = [0u8; 1024];
        let n = stream.read(&mut response).await.expect("Failed to read OK response");
        let ok_response = String::from_utf8_lossy(&response[..n]);
        debug!("Received response: {}", ok_response);
        assert!(ok_response.contains("OK"), "Server should respond with OK");
        
        // Отправляем данные
        let chunks = 3; // Количество чанков
        let mut total_bytes = 0;
        let mut rng = rand::thread_rng();
        
        debug!("Sending {} chunks of {} bytes each", chunks, chunk_size);
        
        // Отправляем чанки
        for i in 0..chunks {
            let mut chunk = vec![0u8; chunk_size];
            
            // Заполняем чанк случайными данными
            rng.fill_bytes(&mut chunk[..chunk_size - 1]);
            
            // Устанавливаем последний байт
            if i == chunks - 1 {
                // Последний чанк имеет терминатор 0xFF
                chunk[chunk_size - 1] = 0xFF;
            } else {
                // Обычные чанки имеют 0x00
                chunk[chunk_size - 1] = 0x00;
            }
            
            // Отправляем чанк
            stream.write_all(&chunk).await.expect("Failed to send chunk");
            stream.flush().await.expect("Failed to flush chunk");
            
            total_bytes += chunk_size;
            debug!("Sent chunk {}/{}: {} bytes, total: {}", 
                   i + 1, chunks, chunk_size, total_bytes);
            
            // Подробное логирование содержимого чанка
            trace!("Chunk data (first 32 bytes): {:?}", &chunk[..std::cmp::min(32, chunk_size)]);
            trace!("Chunk data (last 32 bytes): {:?}", &chunk[chunk_size.saturating_sub(32)..chunk_size]);
            trace!("Last byte: 0x{:02X}", chunk[chunk_size - 1]);
        }
        
        // Проверяем, что отправили данные
        assert!(total_bytes > 0, "Should send some data");

        // Читаем TIME ответ от сервера
        let mut response = [0u8; 1024];
        let n = stream.read(&mut response).await.expect("Failed to read TIME response");
        let time_response = String::from_utf8_lossy(&response[..n]);
        debug!("Received TIME response: {}", time_response);
        assert!(time_response.contains("TIME"), "Server should respond with TIME");

        // Отправляем QUIT команду
        debug!("Sending QUIT command");
        stream.write_all(b"QUIT\n").await.expect("Failed to send QUIT");
        stream.flush().await.expect("Failed to flush QUIT command");

        // Читаем BYE ответ
        let mut response = [0u8; 1024];
        let n = stream.read(&mut response).await.expect("Failed to read BYE response");
        let bye_response = String::from_utf8_lossy(&response[..n]);
        debug!("Received response to QUIT: {}", bye_response);
        assert!(bye_response.contains("BYE"), "Server should respond with BYE");
        
        // Закрываем соединение
        stream.shutdown().await.expect("Failed to shutdown stream");
    });
} 