use crate::test_utils::{ generate_token, TestServer};
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio::io::{AsyncWriteExt, AsyncReadExt};
use log::{info, LevelFilter};
use env_logger::Builder;
use tokio_native_tls::TlsConnector;
use native_tls::TlsConnector as NativeTlsConnector;


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
    
    // Создаем тестовый сервер (он может быть dummy если используем дефолтные порты)
    let server = TestServer::new(None, None).unwrap();

    // Create TLS connector
    let tls_connector = NativeTlsConnector::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap();
    let tls_connector = TlsConnector::from(tls_connector);

    // Connect to the TLS port
    info!("Connecting to TLS port {}", server.tls_port());
    let stream = timeout(
        Duration::from_secs(5),
        TcpStream::connect(format!("127.0.0.1:{}", server.tls_port()))
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
    stream.flush().await.unwrap();

    // Read token response
    let mut buf = [0u8; 1024];
    info!("Waiting for token response...");
    let n = timeout(Duration::from_secs(5), stream.read(&mut buf)).await.unwrap().unwrap();
    let response = String::from_utf8_lossy(&buf[..n]);
    info!("Received token response: {}", response);
    assert!(response.contains("OK"), "Server should accept valid token");
}
