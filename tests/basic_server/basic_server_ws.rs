use crate::test_utils::{find_free_port, generate_token, TestServer};
use log::{info, LevelFilter};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::{Message, handshake::client::Request};
use futures_util::{SinkExt, StreamExt};
use tokio_native_tls::TlsConnector;
use native_tls::TlsConnector as NativeTlsConnector;
use std::env;
use env_logger::Builder;

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
    
    // Создаем тестовый сервер (он может быть dummy если используем дефолтные порты)
    let server = TestServer::new(None, None).unwrap();

    // Create TLS connector with custom configuration
    let mut tls_connector = NativeTlsConnector::builder();
    tls_connector.danger_accept_invalid_certs(true);
    tls_connector.danger_accept_invalid_hostnames(true);
    let tls_connector = TlsConnector::from(tls_connector.build().unwrap());

    // Connect to the TLS port first
    info!("Connecting to TLS port {}", server.tls_port());
    let tcp_stream = TcpStream::connect(format!("{}:{}", server.host(), server.tls_port())).await.unwrap();
    
    // Determine host based on environment variable
    let host = if env::var("TEST_USE_DEFAULT_PORTS").is_ok() {
        "dev.measurementservers.net"
    } else {
        "localhost"
    };
    
    let tls_stream = tls_connector.connect(host, tcp_stream).await.unwrap();

    // Create WebSocket request
    let request = Request::builder()
        .uri(format!("wss://{}:{}/rmbt", host, server.tls_port()))
        .header("Host", format!("{}:{}", host, server.tls_port()))
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

    // Read CHUNKSIZE message
    let chunksize_response = read.next().await.expect("Failed to read CHUNKSIZE").expect("Failed to read CHUNKSIZE");
    let chunksize_text = chunksize_response.to_text().expect("CHUNKSIZE is not text");
    info!("Received CHUNKSIZE message: {}", chunksize_text);
    assert!(chunksize_text.contains("CHUNKSIZE"), "Server should send CHUNKSIZE message");

    // Read ACCEPT message with all commands
    let accept_response = read.next().await.expect("Failed to read ACCEPT").expect("Failed to read ACCEPT");
    let accept_text = accept_response.to_text().expect("ACCEPT is not text");
    info!("Received ACCEPT message: {}", accept_text);
    assert!(accept_text.contains("ACCEPT"), "Server should send ACCEPT message");
    assert!(accept_text.contains("GETCHUNKS"), "Server should accept GETCHUNKS command");
    assert!(accept_text.contains("GETTIME"), "Server should accept GETTIME command");
    assert!(accept_text.contains("PUT"), "Server should accept PUT command");
    assert!(accept_text.contains("PUTNORESULT"), "Server should accept PUTNORESULT command");
    assert!(accept_text.contains("PING"), "Server should accept PING command");
    assert!(accept_text.contains("QUIT"), "Server should accept QUIT command");

    // Close WebSocket connection
    write.close().await.expect("Failed to close WebSocket connection");
    info!("Closed WebSocket connection");
}
