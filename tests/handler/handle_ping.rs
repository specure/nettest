#[path = "../test_utils/mod.rs"]
mod test_utils;

use tokio::runtime::Runtime;
use log::{info, debug, trace};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use crate::test_utils::{find_free_port, TestServer};

#[test]
fn test_handle_ping() {
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
        
        // Отправляем PING команду
        debug!("Sending PING command");
        stream.write_all(b"PING\n").await.expect("Failed to send PING");
        stream.flush().await.expect("Failed to flush PING command");
        
        // Читаем PONG ответ от сервера
        let mut response = [0u8; 1024];
        let n = stream.read(&mut response).await.expect("Failed to read PONG response");
        let pong_response = String::from_utf8_lossy(&response[..n]);
        debug!("Received response: {}", pong_response);
        assert!(pong_response.contains("PONG"), "Server should respond with PONG");
        
        // Отправляем OK
        debug!("Sending OK response");
        stream.write_all(b"OK\n").await.expect("Failed to send OK");
        stream.flush().await.expect("Failed to flush OK response");
        
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