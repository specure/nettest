use crate::test_utils::{find_free_port, generate_token, TestServer};
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio::io::{AsyncWriteExt, AsyncReadExt};
use std::env;
use log::{debug, info, LevelFilter};
use env_logger::Builder;
use tokio_native_tls::TlsConnector;
use native_tls::TlsConnector as NativeTlsConnector;

const DEFAULT_PLAIN_PORT: u16 = 8080;
const DEFAULT_TLS_PORT: u16 = 443;

fn setup_logging() {
    let _ = Builder::new()
        .filter_level(LevelFilter::Debug)
        .format_timestamp_millis()
        .format_module_path(false)
        .format_target(false)
        .try_init();
}

#[tokio::test]
async fn test_rmbt_upgrade() {
    setup_logging();
    // Get ports from environment or use random free ports
    let (plain_port, tls_port) = if env::var("TEST_USE_DEFAULT_PORTS").is_ok() {
        (DEFAULT_PLAIN_PORT, DEFAULT_TLS_PORT)
    } else {
        (find_free_port(), find_free_port())
    };

    info!("Using ports: plain={}, tls={}", plain_port, tls_port);

    // Start server only if not using default ports
    let _server = if plain_port != DEFAULT_PLAIN_PORT || tls_port != DEFAULT_TLS_PORT {
        Some(TestServer::new(plain_port, tls_port))
    } else {
        None
    };

    // Create TLS connector
    let tls_connector = NativeTlsConnector::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap();
    let tls_connector = TlsConnector::from(tls_connector);

    // Connect to the TLS port
    info!("Connecting to TLS port {}", tls_port);
    let stream = timeout(
        Duration::from_secs(5),
        TcpStream::connect(format!("127.0.0.1:{}", tls_port))
    ).await.unwrap().unwrap();
    info!("Connected to server");

    // Upgrade to TLS
    let mut stream = tls_connector.connect("localhost", stream).await.unwrap();
    info!("TLS connection established");

    // Send RMBT upgrade request
    let upgrade_request = "GET /rmbt HTTP/1.1 \r\n\
                         Connection: Upgrade \r\n\
                         Upgrade: RMBT\r\n\
                         RMBT-Version: 1.2.0\r\n\
                         \r\n";

    info!("Sending RMBT upgrade request: {}", upgrade_request);
    let written = stream.write(upgrade_request.as_bytes()).await.unwrap();
    assert_eq!(written, upgrade_request.len(), "Not all bytes were written");
    stream.flush().await.unwrap();
    info!("RMBT upgrade request sent");

    // Read and verify upgrade response
    let mut buf = [0u8; 1024];
    info!("Waiting for upgrade response...");
    let n = timeout(Duration::from_secs(5), stream.read(&mut buf)).await.unwrap().unwrap();
    let response = String::from_utf8_lossy(&buf[..n]);
    info!("Received RMBT upgrade response: {}", response);
    
    assert!(response.contains("101 Switching Protocols"), "Server should accept RMBT upgrade");
    assert!(response.contains("Upgrade: RMBT"), "Server should upgrade to RMBT");

    // Read greeting message
    let mut buf = [0u8; 1024];
    info!("Waiting for greeting message...");
    let n = timeout(Duration::from_secs(5), stream.read(&mut buf)).await.unwrap().unwrap();
    let greeting = String::from_utf8_lossy(&buf[..n]);
    info!("Received greeting: {}", greeting);
    assert!(greeting.contains("RMBTv"), "Server should send version in greeting");

    // Read ACCEPT TOKEN message
    let mut buf = [0u8; 1024];
    info!("Waiting for ACCEPT TOKEN message...");
    let n = timeout(Duration::from_secs(5), stream.read(&mut buf)).await.unwrap().unwrap();
    let accept_token = String::from_utf8_lossy(&buf[..n]);
    info!("Received ACCEPT TOKEN message: {}", accept_token);
    assert!(accept_token.contains("ACCEPT TOKEN"), "Server should send ACCEPT TOKEN message");

    // Send token
    let token = generate_token().expect("Failed to generate token");
    info!("Sending token: {}", token);
    stream.write_all(token.as_bytes()).await.unwrap();
    stream.write_all(b"\n").await.unwrap();
    stream.flush().await.unwrap();

    // Read token response
    let mut buf = [0u8; 1024];
    let n = timeout(Duration::from_secs(5), stream.read(&mut buf)).await.unwrap().unwrap();
    let response = String::from_utf8_lossy(&buf[..n]);
    info!("Received token response: {}", response);
    
    assert!(response.contains("OK"), "Server should accept valid token");

}
