use crate::stream::Stream;
use crate::stream::Stream::{Plain, Tls};
use crate::utils::websocket::{generate_handshake_response, Handshake};
use log::{debug, error, info};
use regex::Regex;
use std::error::Error;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio_native_tls::TlsAcceptor;

pub const MAX_LINE_LENGTH: usize = 1024;
const RMBT_UPGRADE: &str = "HTTP/1.1 101 Switching Protocols\r\nUpgrade: RMBT\r\nConnection: Upgrade\r\n\r\n";

pub async fn define_stream(
    tcp_stream: TcpStream,
    tls_acceptor: Option<Arc<TlsAcceptor>>,
) -> Result<Stream, Box<dyn Error + Send + Sync>> {
    info!("Handling HTTP request");

    let mut stream: Stream;

    if let Some(acceptor) = tls_acceptor {
        stream = Tls(acceptor.accept(tcp_stream).await?);
    } else {
        stream = Plain(tcp_stream);
    }

    let mut buffer = [0u8; MAX_LINE_LENGTH];
    let n = stream.read(&mut buffer).await?;

    if n < 4 {
        error!("Received data too short: {} bytes", n);
        return Err("Invalid request: data too short".into());
    }

    let is_get = buffer[0] == b'G' && buffer[1] == b'E' && buffer[2] == b'T' && buffer[3] == b' ';

    if !is_get {
        error!("Not a GET request");
        return Err("Invalid request: not a GET request".into());
    }
    let request = String::from_utf8_lossy(&buffer[..n]);

    let ws_regex = Regex::new(r"(?i)upgrade:\s*websocket").unwrap();
    let rmbt_regex = Regex::new(r"(?i)upgrade:\s*rmbt").unwrap();

    let is_websocket = ws_regex.is_match(&request);
    let is_rmbt = rmbt_regex.is_match(&request);

    if !is_websocket && !is_rmbt {
        debug!("No HTTP upgrade to websocket/rmbt");
        return Ok(stream);
    }

    if is_rmbt {
        debug!("Upgrading to RMBT");
        stream.write_all(RMBT_UPGRADE.as_bytes()).await?;
        stream.flush().await?;
        return Ok(stream);
    }

    if is_websocket {
        info!("Upgrading to WebSocket");

        // Parse WebSocket handshake
        let handshake = Handshake::parse(&request)?;
        if !handshake.is_valid() {
            error!("Invalid WebSocket handshake");
            return Err("Invalid WebSocket handshake".into());
        }

        // Generate and send handshake response
        let response = generate_handshake_response(&handshake)?;
        stream.write_all(response.as_bytes()).await?;
        stream.flush().await?;

        debug!("Upgrading to WebSocket");

        match stream.upgrade_to_websocket().await {
            Ok(ws_stream) => {
                debug!("WebSocket upgraded");
                Ok(ws_stream)
            }
            Err(e) => {
                error!("WebSocket upgrade failed: {}", e);
                Err(Box::new(e) as Box<dyn Error + Send + Sync>)
            }
        }
    } else {
        debug!("No HTTP upgrade to websocket/rmbt");
        Ok(stream)
    }
}
