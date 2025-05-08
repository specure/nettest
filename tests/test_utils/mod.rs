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
use std::sync::{Mutex};
use lazy_static::lazy_static;
use std::sync::atomic::{AtomicBool, Ordering, AtomicUsize};
use libc::atexit;

const DEFAULT_PLAIN_PORT: u16 = 8080;
const DEFAULT_TLS_PORT: u16 = 443;
// const DEFAULT_HOST: &str = "dev.measurementservers.net";
const DEFAULT_HOST: &str = "127.0.0.1";


type HmacSha1 = Hmac<Sha1>;

lazy_static! {
    static ref TEST_SERVER: Mutex<Option<TestServerInstance>> = Mutex::new(None);
    static ref SERVER_INITIALIZED: AtomicBool = AtomicBool::new(false);
    static ref ACTIVE_TESTS: AtomicUsize = AtomicUsize::new(0);
}

struct TestServerInstance {
    process: Child,
    plain_port: u16,
    tls_port: u16,
    host: String,
}

pub struct TestServer {
    plain_port: u16,
    tls_port: u16,
    host: String,
}

extern "C" fn cleanup_server() {
    info!("Cleaning up server on exit");
    if let Some(mut server) = TEST_SERVER.lock().unwrap().take() {
        // Kill process on ports
        let output = Command::new("lsof")
            .args(["-i", &format!(":{}", server.plain_port), "-t"])
            .output()
            .expect("Failed to execute lsof");

        if let Ok(pid_str) = String::from_utf8(output.stdout) {
            if let Some(pid) = pid_str.trim().parse::<i32>().ok() {
                Command::new("kill")
                    .args(["-9", &pid.to_string()])
                    .output()
                    .expect("Failed to kill process");
                info!("Killed process {} on port {}", pid, server.plain_port);
            }
        }

        let output = Command::new("lsof")
            .args(["-i", &format!(":{}", server.tls_port), "-t"])
            .output()
            .expect("Failed to execute lsof");

        if let Ok(pid_str) = String::from_utf8(output.stdout) {
            if let Some(pid) = pid_str.trim().parse::<i32>().ok() {
                Command::new("kill")
                    .args(["-9", &pid.to_string()])
                    .output()
                    .expect("Failed to kill process");
                info!("Killed process {} on port {}", pid, server.tls_port);
            }
        }

        // Wait for process to finish
        let _ = server.process.wait();
    }
}

impl TestServer {
    pub fn new(plain_port: Option<u16>, tls_port: Option<u16>) -> Option<Self> {
        // Increment active tests counter
        ACTIVE_TESTS.fetch_add(1, Ordering::SeqCst);

        // Quick check if server exists without holding the lock
        if SERVER_INITIALIZED.load(Ordering::SeqCst) {
            let server = TEST_SERVER.lock().unwrap();
            if let Some(instance) = server.as_ref() {
                return Some(Self {
                    plain_port: instance.plain_port,
                    tls_port: instance.tls_port,
                    host: instance.host.clone(),
                });
            }
        }

        // If we get here, we need to create a new server
        let mut server = TEST_SERVER.lock().unwrap();

        // Double check if server was created while we were waiting for the lock
        if let Some(instance) = server.as_ref() {
            return Some(Self {
                plain_port: instance.plain_port,
                tls_port: instance.tls_port,
                host: instance.host.clone(),
            });
        }

        // Define ports
        let port = if env::var("TEST_USE_DEFAULT_PORTS").is_ok() { DEFAULT_PLAIN_PORT } else { find_free_port() };
        let ws_port = if env::var("TEST_USE_DEFAULT_PORTS").is_ok() { DEFAULT_TLS_PORT } else { find_free_port() };

        // If using default ports - don't create server
        if env::var("TEST_USE_DEFAULT_PORTS").is_ok() {
            let instance = TestServerInstance {
                process: Command::new("echo").spawn().unwrap(), // dummy process
                plain_port: port,
                tls_port: ws_port,
                host: DEFAULT_HOST.to_string(),
            };
            *server = Some(instance);
            SERVER_INITIALIZED.store(true, Ordering::SeqCst);
            return Some(Self {
                plain_port: port,
                tls_port: ws_port,
                host: DEFAULT_HOST.to_string(),
            });
        }

        info!("Starting test server on ports: {} (plain) and {} (tls)", port, ws_port);

        let process = Command::new("cargo")
            .args([
                "run", "--",
                "-l", &port.to_string(),
                "-D",
                "-L", &ws_port.to_string(),
                "-c", "measurementservers.crt",
                "-k", "measurementservers.key",
                "-w"
            ])
            .spawn()
            .expect("Failed to start server");

        info!("Server process started with PID: {}", process.id());

        // Give server time to start
        thread::sleep(Duration::from_secs(2));

        let instance = TestServerInstance {
            process,
            plain_port: port,
            tls_port: ws_port,
            host: "127.0.0.1".to_string(),
        };

        *server = Some(instance);
        SERVER_INITIALIZED.store(true, Ordering::SeqCst);

        // Register cleanup function on first server creation
        unsafe {
            atexit(cleanup_server);
        }

        Some(Self {
            plain_port: port,
            tls_port: ws_port,
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
        // Decrement active tests counter
        let remaining = ACTIVE_TESTS.fetch_sub(1, Ordering::SeqCst);

        // If this was the last test, cleanup the server
        if remaining == 1 {
            if let Some(mut server) = TEST_SERVER.lock().unwrap().take() {
                let _ = server.process.kill();
                let _ = server.process.wait();
            }
            SERVER_INITIALIZED.store(false, Ordering::SeqCst);
        }
    }
}

pub fn find_free_port() -> u16 {
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, 0));
    let listener = TcpListener::bind(addr).unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    port
}

// Use server key
const SERVER_KEY: &str = "PWOGfvIOPbvnltvjKHbmU8WGR5ycbEH55htjWDb5t0EhcKqFFgFwLm9tP3uQB5iJ3BwT4tZADvaW9LR8eYSY6OgCPW6dXQxQzdjh";

pub fn generate_token() -> Result<String, Box<dyn Error + Send + Sync>> {
    // Generate UUID and verify its format (36 characters, only hex and dashes)
    let uuid = Uuid::new_v4().to_string();
    if uuid.len() != 36 || !uuid.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
        return Err("Invalid UUID format".into());
    }

    // Get current time in seconds
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)?
        .as_secs();

    // Compensate time difference only when using default ports (real server)
    let start_time = if env::var("TEST_USE_DEFAULT_PORTS").is_ok() {
        // Subtract 30 seconds to compensate for server time difference
        (now - 30).to_string()
    } else {
        now.to_string()
    };

    // Verify time format (digits only)
    if !start_time.chars().all(|c| c.is_ascii_digit()) {
        return Err("Invalid time format".into());
    }

    // Form message for HMAC (max 128 bytes as in C code)
    let message = format!("{}_{}", uuid, start_time);
    if message.len() > 128 {
        return Err("Message too long".into());
    }

    // Generate HMAC-SHA1 as in Java code
    let mut mac = HmacSha1::new_from_slice(SERVER_KEY.as_bytes())?;
    mac.update(message.as_bytes());
    let result = mac.finalize();
    let code_bytes = result.into_bytes();

    // Encode in base64 as in Java code
    let hmac = BASE64.encode(code_bytes);

    // Verify HMAC format (base64)
    if !hmac.chars().all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=') {
        return Err("Invalid HMAC format".into());
    }

    // Form token in the same format as C code
    let token = format!("TOKEN {}_{}_{}\n", uuid, start_time, hmac);

    // Verify token format
    if !token.starts_with("TOKEN ") {
        return Err("Invalid token format: must start with 'TOKEN '".into());
    }

    let token_parts: Vec<&str> = token[6..].trim().split('_').collect();
    if token_parts.len() != 3 {
        return Err("Invalid token format: must be TOKEN uuid_starttime_hmac".into());
    }

    Ok(token)
}

pub fn create_optimized_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(200)
        .enable_all()
        .thread_name("rmbt-worker")
        .thread_stack_size(30 * 1024 * 1024) // 3MB stack size
        .build()
        .expect("Failed to create Tokio runtime")
}
