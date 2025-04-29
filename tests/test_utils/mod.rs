use std::process::{Command, Child};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use log::info;
use tokio::net::TcpStream;
use uuid::Uuid;
use tokio_tungstenite::tungstenite::{Message, handshake::client::Request};
use tokio_tungstenite::WebSocketStream;
use futures_util::{SinkExt, StreamExt};
use tokio_native_tls::TlsConnector;
use native_tls::TlsConnector as NativeTlsConnector;
use std::net::{TcpListener, SocketAddr};
use std::net::Ipv4Addr;
use tokio_native_tls::TlsStream;
use std::error::Error;
use hmac::{Hmac, Mac};
use sha1::Sha1;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use std::env;

const DEFAULT_PLAIN_PORT: u16 = 8080;
const DEFAULT_TLS_PORT: u16 = 443;

type HmacSha1 = Hmac<Sha1>;

pub struct TestServer {
    process: Child,
    plain_port: u16,
    tls_port: u16,
    host: String,
}

impl TestServer {
    pub fn new(plain_port: Option<u16>, tls_port: Option<u16>) -> Option<Self> {
        // Определяем порты в зависимости от переменной окружения
        let (plain_port, tls_port) = if env::var("TEST_USE_DEFAULT_PORTS").is_ok() {
            (DEFAULT_PLAIN_PORT, DEFAULT_TLS_PORT)
        } else {
            (plain_port.unwrap_or_else(find_free_port), tls_port.unwrap_or_else(find_free_port))
        };

        info!("Using ports: plain={}, tls={}", plain_port, tls_port);

        // Если используем дефолтные порты - не создаем сервер
        if env::var("TEST_USE_DEFAULT_PORTS").is_ok() {
            return Some(Self {
                process: Command::new("echo").spawn().unwrap(), // dummy process
                plain_port,
                tls_port,
                host: "dev.measurementservers.net".to_string(),
            });
        }

        info!("Starting test server on ports: {} (plain) and {} (tls)", plain_port, tls_port);

        let process = Command::new("cargo")
            .args([
                "run", "--",
                "-l", &plain_port.to_string(),
                "-D",
                "-L", &tls_port.to_string(),
                "-c", "measurementservers.crt",
                "-k", "measurementservers.key",
                "-w"
            ])
            .spawn()
            .expect("Failed to start server");

        info!("Server process started with PID: {}", process.id());

        // Give server time to start
        thread::sleep(Duration::from_secs(2));

        Some(Self {
            process,
            plain_port,
            tls_port,
            host: "127.0.0.1".to_string(),
        })
    }

    pub fn plain_port(&self) -> u16 {
        self.plain_port
    }

    pub fn tls_port(&self) -> u16 {
        self.tls_port
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    pub async fn connect_ws(&self) -> Result<(WebSocketStream<TlsStream<TcpStream>>, u32), Box<dyn Error + Send + Sync>> {
        // Create TLS connector with custom configuration
        let mut tls_connector = NativeTlsConnector::builder();
        tls_connector.danger_accept_invalid_certs(true);
        tls_connector.danger_accept_invalid_hostnames(true);
        let tls_connector = TlsConnector::from(tls_connector.build().unwrap());

        // Connect to the TLS port
        info!("Connecting to TLS port {}", self.tls_port);
        let tcp_stream = TcpStream::connect(format!("{}:{}", self.host, self.tls_port)).await?;
        let tls_stream = tls_connector.connect("localhost", tcp_stream).await?;

        // Create WebSocket request
        let request = Request::builder()
            .uri(format!("wss://127.0.0.1:{}/rmbt", self.tls_port))
            .header("Host", format!("127.0.0.1:{}", self.tls_port))
            .header("Connection", "Upgrade")
            .header("Upgrade", "websocket")
            .header("Sec-WebSocket-Version", "13")
            .header("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ==")
            .body(())
            .unwrap();

        // Upgrade TLS connection to WebSocket
        let (ws_stream, _) = tokio_tungstenite::client_async(request, tls_stream).await?;
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

        // Send token
        let token = generate_token()?;
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

        // Parse chunk size
        let chunk_size = chunksize_text
            .split_whitespace()
            .nth(1)
            .unwrap()
            .parse::<u32>()
            .expect("Failed to parse chunk size");

        // Recombine the stream
        let ws_stream = write.reunite(read).expect("Failed to reunite WebSocket stream");

        Ok((ws_stream, chunk_size))
    }

    pub async fn connect_rmbtd(&self) -> Result<(TlsStream<TcpStream>, u32), Box<dyn Error + Send + Sync>> {
        // Create TLS connector with custom configuration
        let mut tls_connector = NativeTlsConnector::builder();
        tls_connector.danger_accept_invalid_certs(true);
        tls_connector.danger_accept_invalid_hostnames(true);
        let tls_connector = TlsConnector::from(tls_connector.build().unwrap());

        // Connect to the TLS port
        info!("Connecting to TLS port {}", self.tls_port);
        let tcp_stream = TcpStream::connect(format!("{}:{}", self.host, self.tls_port)).await?;
        let mut tls_stream = tls_connector.connect("localhost", tcp_stream).await?;

        // Send RMBT upgrade request
        let upgrade_request = "GET /rmbt HTTP/1.1 \r\n\
                             Connection: Upgrade \r\n\
                             Upgrade: RMBT\r\n\
                             RMBT-Version: 1.2.0\r\n\
                             \r\n";

        info!("Sending RMBT upgrade request: {}", upgrade_request);
        tls_stream.write_all(upgrade_request.as_bytes()).await?;
        tls_stream.flush().await?;

        // Read and verify upgrade response
        let mut buf = [0u8; 1024];
        info!("Waiting for upgrade response...");
        let n = tls_stream.read(&mut buf).await?;
        let response = String::from_utf8_lossy(&buf[..n]);
        info!("Received RMBT upgrade response: {}", response);
        
        assert!(response.contains("101 Switching Protocols"), "Server should accept RMBT upgrade");
        assert!(response.contains("Upgrade: RMBT"), "Server should upgrade to RMBT");

        // Read greeting message
        let mut buf = [0u8; 1024];
        info!("Waiting for greeting message...");
        let n = tls_stream.read(&mut buf).await?;
        let greeting = String::from_utf8_lossy(&buf[..n]);
        info!("Received greeting: {}", greeting);
        assert!(greeting.contains("RMBTv"), "Server should send version in greeting");

        // Read ACCEPT TOKEN message
        let mut buf = [0u8; 1024];
        info!("Waiting for ACCEPT TOKEN message...");
        let n = tls_stream.read(&mut buf).await?;
        let accept_token = String::from_utf8_lossy(&buf[..n]);
        info!("Received ACCEPT TOKEN message: {}", accept_token);
        assert!(accept_token.contains("ACCEPT TOKEN"), "Server should send ACCEPT TOKEN message");

        // Send token
        let token = generate_token()?;
        info!("Sending token: {}", token);
        tls_stream.write_all(token.as_bytes()).await?;
        tls_stream.flush().await?;

        // Read token response
        let mut buf = [0u8; 1024];
        info!("Waiting for token response...");
        let n = tls_stream.read(&mut buf).await?;
        let response = String::from_utf8_lossy(&buf[..n]);
        info!("Received token response: {}", response);
        assert!(response.contains("OK"), "Server should accept valid token");

        // Read CHUNKSIZE message
        let mut buf = [0u8; 1024];
        info!("Waiting for CHUNKSIZE message...");
        let n = tls_stream.read(&mut buf).await?;
        let chunksize_msg = String::from_utf8_lossy(&buf[..n]);
        info!("Received CHUNKSIZE message: {}", chunksize_msg);
        assert!(chunksize_msg.contains("CHUNKSIZE"), "Server should send CHUNKSIZE message");

        // Parse chunk size
        let chunk_size = chunksize_msg
            .split_whitespace()
            .nth(1)
            .unwrap()
            .parse::<u32>()
            .expect("Failed to parse chunk size");

        Ok((tls_stream, chunk_size))
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        // Kill process on ports
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

        // Wait for process to finish
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

const TEST_KEY: &str = "q4aFShYnBgoYyDr4cxes0DSYCvjLpeKJjhCfvmVCdiIpsdeU1djvBtE6CMtNCbDWkiU68X7bajIAwLon14Hh7Wpi5MJWJL7HXokh";

pub fn generate_token() -> Result<String, Box<dyn Error + Send + Sync>> {
    // Генерируем UUID
    let uuid = Uuid::new_v4().to_string();
    
    // Получаем текущее время
    let start_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)?
        .as_secs()
        .to_string();
    
    // Генерируем HMAC
    let message = format!("{}_{}", uuid, start_time);
    let mut mac = HmacSha1::new_from_slice(TEST_KEY.as_bytes())?;
    mac.update(message.as_bytes());
    let result = mac.finalize();
    let code_bytes = result.into_bytes();
    let hmac = BASE64.encode(code_bytes);
    
    // Формируем токен
    Ok(format!("TOKEN {}_{}_{}\n", uuid, start_time, hmac))
}
