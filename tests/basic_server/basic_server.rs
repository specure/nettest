#[path = "../test_utils/mod.rs"]
mod test_utils;

use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use std::net::{TcpListener, TcpStream};
use std::process::{Command, Output};
use std::thread;
use std::time::Duration;
use log::{debug, info};
use tokio::time::sleep;
use tokio::net::TcpStream as TokioTcpStream;
use tokio::runtime::Runtime;
use uuid::Uuid;
use std::time::{SystemTime, UNIX_EPOCH};
use hmac::{Hmac, Mac};
use sha1::Sha1;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use std::fs;
use crate::test_utils::{find_free_port, TestServer};


type HmacSha1 = Hmac<Sha1>;

fn generate_token(key: &str) -> String {
    let uuid = Uuid::new_v4().to_string();
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        .to_string();
    
    let message = format!("{}_{}", uuid, timestamp);
    let mut mac = HmacSha1::new_from_slice(key.as_bytes()).unwrap();
    mac.update(message.as_bytes());
    let result = mac.finalize();
    let code_bytes = result.into_bytes();
    let hmac = BASE64.encode(code_bytes);
    
    format!("{}_{}_{}", uuid, timestamp, hmac)
}

fn kill_process_on_port(port: u16) {
    let output = Command::new("lsof")
        .args(["-i", &format!(":{}", port), "-t"])
        .output()
        .expect("Failed to execute lsof");
    
    if let Ok(pid_str) = String::from_utf8(output.stdout) {
        if let Some(pid) = pid_str.trim().parse::<i32>().ok() {
            Command::new("kill")
                .args(["-9", &pid.to_string()])
                .output()
                .expect("Failed to kill process");
            info!("Killed process {} on port {}", pid, port);
        }
    }
}

#[test]
fn test_server_starts_with_command() {
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
        println!("Received version: {}", version);
        assert!(version.contains("RMBTv"), "Server greeting should contain version");
        assert!(version.contains("ACCEPT"), "Server greeting should contain ACCEPT");
        assert!(version.contains("TOKEN"), "Server greeting should contain ACCEPT");
        assert!(version.contains("QUIT"), "Server greeting should contain ACCEPT");

        // Отправляем токен
        TestServer::send_token(&mut stream, key).await.expect("Failed to send token");
        
        // Отправляем QUIT
        TestServer::send_quit(&mut stream).await.expect("Failed to send QUIT");
        
        // Закрываем соединение
        stream.shutdown().await.expect("Failed to shutdown stream");
    });
} 