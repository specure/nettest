use std::process::{Command, Child};
use std::net::TcpStream;
use std::thread;
use std::time::Duration;
use log::{info, debug};
use tokio::net::TcpStream as TokioTcpStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use uuid::Uuid;
use std::time::{SystemTime, UNIX_EPOCH};
use hmac::{Hmac, Mac};
use sha1::Sha1;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use std::net::{TcpListener, SocketAddr};
use std::net::Ipv4Addr;

type HmacSha1 = Hmac<Sha1>;

pub struct TestServer {
    process: Child,
    plain_port: u16,
    tls_port: u16,
}

impl TestServer {
    pub fn new(plain_port: u16, tls_port: u16) -> Self {
        info!("Starting test server on ports: {} (plain) and {} (tls)", plain_port, tls_port);

        let process = Command::new("cargo")
            .args([
                "run", "--",
                "-l", &plain_port.to_string(),
                "-D",
                "-L", &tls_port.to_string(),
                "-c", "measurementservers.crt",
                "-k", "measurementservers.key"
            ])
            .spawn()
            .expect("Failed to start server");

        // Даем серверу время на запуск
        thread::sleep(Duration::from_secs(2));

        Self {
            process,
            plain_port,
            tls_port,
        }
    }

    pub fn generate_token(key: &str) -> String {
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

    pub async fn connect(&self, use_tls: bool) -> Result<TokioTcpStream, Box<dyn std::error::Error>> {
        let port = if use_tls { self.tls_port } else { self.plain_port };
        let stream = TokioTcpStream::connect(format!("127.0.0.1:{}", port))
            .await
            .expect("Failed to connect to server");
        Ok(stream)
    }

    pub async fn send_token(stream: &mut TokioTcpStream, key: &str) -> Result<(), Box<dyn std::error::Error>> {
        let token = Self::generate_token(key);
        debug!("Sending token: {}", token);
        stream.write_all(format!("TOKEN {}\n", token).as_bytes())
            .await?;

        let mut buf = [0u8; 1024];
        let n = stream.read(&mut buf).await?;
        let response = String::from_utf8_lossy(&buf[..n]);
        debug!("Received token response: {}", response);

        assert!(response.contains("OK"), "Server should accept valid token");
        Ok(())
    }

    pub async fn send_quit(stream: &mut TokioTcpStream) -> Result<(), Box<dyn std::error::Error>> {
        stream.write_all(b"QUIT\n").await?;

        let mut buf = [0u8; 1024];
        let n = stream.read(&mut buf).await?;
        let response = String::from_utf8_lossy(&buf[..n]);
        debug!("Received QUIT response: {}", response);

        assert!(response.contains("BYE"), "Server should respond with BYE");
        Ok(())
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        // Убиваем процесс на портах
        let output = Command::new("lsof")
            .args(["-i", &format!(":{}", self.plain_port), "-t"])
            .output()
            .expect("Failed to execute lsof");

        if let Ok(pid_str) = String::from_utf8(output.stdout) {
            if let Some(pid) = pid_str.trim().parse::<i32>().ok() {
                Command::new("kill")
                    .args(["-9", &pid.to_string()])
                    .output()
                    .expect("Failed to kill process");
                info!("Killed process {} on port {}", pid, self.plain_port);
            }
        }

        let output = Command::new("lsof")
            .args(["-i", &format!(":{}", self.tls_port), "-t"])
            .output()
            .expect("Failed to execute lsof");

        if let Ok(pid_str) = String::from_utf8(output.stdout) {
            if let Some(pid) = pid_str.trim().parse::<i32>().ok() {
                Command::new("kill")
                    .args(["-9", &pid.to_string()])
                    .output()
                    .expect("Failed to kill process");
                info!("Killed process {} on port {}", pid, self.tls_port);
            }
        }

        // Ждем завершения процесса
        let _ = self.process.wait();
    }
}

pub fn find_free_port() -> u16 {
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, 0));
    let listener = TcpListener::bind(addr).unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    port
} 