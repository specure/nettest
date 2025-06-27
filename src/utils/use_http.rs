use crate::stream::Stream;
use crate::stream::Stream::{Plain, Tls};
use crate::utils::websocket::{generate_handshake_response, Handshake};
use log::{debug, info};
use regex::Regex;
use std::error::Error;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio_rustls::TlsAcceptor;

pub const MAX_LINE_LENGTH: usize = 1024;
const RMBT_UPGRADE: &str = "HTTP/1.1 101 Switching Protocols\r\nConnection: Upgrade\r\nUpgrade: RMBT\r\n\r\n";


pub async fn define_stream(
    tcp_stream: TcpStream,
    tls_acceptor: Option<Arc<TlsAcceptor>>,
    is_ssl: bool,
) -> Result<Stream, Box<dyn Error + Send + Sync>> {
    info!("Handling HTTP request");

    let mut stream: Stream;

    if is_ssl {
        if let Some(acceptor) = tls_acceptor {
            debug!("Upgrading to TLS connection");
            stream = Tls(acceptor.accept(tcp_stream).await?);
            debug!("TLS connection established");
        } else {
            // error!("No TLS acceptor");
            return Err("No TLS acceptor".into());
        }
    } else {
        debug!("Using plain TCP connection");
        stream = Plain(tcp_stream);
    }

    let mut buffer = [0u8; MAX_LINE_LENGTH];
    debug!("Reading initial request data");
    let n = stream.read(&mut buffer).await?;
    debug!("Read {} bytes from stream", n);
    
    if n < 4 {
        // error!("Received data too short: {} bytes", n);
        return Err("Invalid request: data too short".into());
    }

    let is_get = buffer[0] == b'G' && buffer[1] == b'E' && buffer[2] == b'T' && buffer[3] == b' ';
    debug!("Request is GET: {}", String::from_utf8_lossy(&buffer[..n]));

    if !is_get {
        // error!("Not a GET request");
        return Err("Invalid request: not a GET request".into());
    }
    let request = String::from_utf8_lossy(&buffer[..n]);
    debug!("Received request: {}", request);

    let ws_regex = Regex::new(r"(?i)upgrade:\s*websocket").unwrap();
    let rmbt_regex = Regex::new(r"(?i)upgrade:\s*rmbt").unwrap();

    let is_websocket = ws_regex.is_match(&request);
    let is_rmbt = rmbt_regex.is_match(&request);
    debug!("WebSocket upgrade requested: {}", is_websocket);
    debug!("RMBT upgrade requested: {}", is_rmbt);

    if !is_websocket && !is_rmbt {
        debug!("No HTTP upgrade to websocket/rmbt");
        return Ok(stream);
    }

    if is_rmbt {
        debug!("Upgrading to RMBT");
        stream.write_all(RMBT_UPGRADE.as_bytes()).await?;
        debug!("RMBT upgrade response sent");
        return Ok(stream);
    }

    if is_websocket {
        info!("Upgrading to WebSocket");

        // Parse WebSocket handshake
        debug!("Parsing WebSocket handshake");
        let handshake = Handshake::parse(&request)?;
        if !handshake.is_valid() {
            // error!("Invalid WebSocket handshake");
            return Err("Invalid WebSocket handshake".into());
        }
        debug!("WebSocket handshake parsed successfully");

        // Generate and send handshake response
        debug!("Generating WebSocket handshake response");
        let response = generate_handshake_response(&handshake)?;
        stream.write_all(response.as_bytes()).await?;
        debug!("WebSocket handshake response sent");

        debug!("Upgrading to WebSocket");
        match stream.upgrade_to_websocket().await {
            Ok(ws_stream) => {
                debug!("WebSocket upgraded successfully");
                Ok(ws_stream)
            }
            Err(e) => {
                // error!("WebSocket upgrade failed: {}", e);
                Err(Box::new(e) as Box<dyn Error + Send + Sync>)
            }
        }
    } else {
        debug!("No HTTP upgrade to websocket/rmbt");
        Ok(stream)
    }
}
