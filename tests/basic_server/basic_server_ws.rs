use crate::test_utils::{find_free_port, generate_token, TestServer};
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio::io::{AsyncWriteExt, AsyncReadExt};
use std::env;
use log::{debug, info, LevelFilter};
use env_logger::Builder;
use tokio_tungstenite::{WebSocketStream, MaybeTlsStream};
use tokio_tungstenite::tungstenite::{Message, handshake::client::Request};
use futures_util::{SinkExt, StreamExt};
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
async fn test_ws_upgrade() {
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

    // Create TLS connector with custom configuration
    let mut tls_connector = NativeTlsConnector::builder();
    tls_connector.danger_accept_invalid_certs(true);
    tls_connector.danger_accept_invalid_hostnames(true);
    let tls_connector = TlsConnector::from(tls_connector.build().unwrap());

    // Connect to the TLS port first
    info!("Connecting to TLS port {}", tls_port);
    let tcp_stream = TcpStream::connect(format!("127.0.0.1:{}", tls_port)).await.unwrap();
    let tls_stream = tls_connector.connect("localhost", tcp_stream).await.unwrap();

    // Create WebSocket request
    let request = Request::builder()
        .uri(format!("wss://127.0.0.1:{}/rmbt", tls_port))
        .header("Host", format!("127.0.0.1:{}", tls_port))
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

    // Send token
    let token = generate_token().expect("Failed to generate token");
    info!("Sending token: {}", token);
    write.send(Message::Text(token)).await.expect("Failed to send token");

    // Read token response
    let response = read.next().await.expect("Failed to read token response").expect("Failed to read token response");
    let response_text = response.to_text().expect("Response is not text");
    info!("Received token response: {}", response_text);
    
    assert!(response_text.contains("OK"), "Server should accept valid token");

    // Close WebSocket connection
    write.close().await.expect("Failed to close WebSocket connection");
    info!("Closed WebSocket connection");
}
