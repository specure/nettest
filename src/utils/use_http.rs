use crate::server::connection_handler::Stream;
use crate::utils::websocket::{generate_handshake_response, Handshake};
use log::{debug, error, info, trace};
use regex::Regex;
use std::error::Error;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_native_tls::TlsAcceptor;
use tokio_tungstenite;

pub const MAX_LINE_LENGTH: usize = 1024;
const CONNECTION_UPGRADE: &str = "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo=\r\n\r\n";
const RMBT_UPGRADE: &str = "HTTP/1.1 101 Switching Protocols\r\nUpgrade: rmbt\r\nConnection: Upgrade\r\n\r\n";
const GREETING: &str = "RMBTv1.2.0\n";
const ACCEPT_TOKEN_NL: &str = "ACCEPT TOKEN QUIT\r\n";

pub async fn define_stream(mut stream: TcpStream, tls_acceptor: Option<Arc<TlsAcceptor>>) -> Result<Stream, Box<dyn Error + Send + Sync>> {
    info!("Handling HTTP request");

    // Читаем только первый байт для проверки на TLS handshake
    // let mut first_byte = [0u8; 1];
    // let n = stream.peek(&mut first_byte).await?;
    let mut isSSL = false;

    // if n > 0 && first_byte[0] == 0x16 {
    //     info!("Received TLS handshake instead of HTTP request");
    //     isSSL = true;
    //     stream.read(&mut first_byte).await?;
    // }

    // Читаем HTTP запрос
    let mut buffer = [0u8; MAX_LINE_LENGTH];
    let n = stream.read(&mut buffer).await?;

    if n <= 0 {
        error!("initialization error: connection reset rmbtws: {} {}", n, std::io::Error::last_os_error());
        return Ok(Stream::Plain(stream)); //todo error
    }

    // Выводим полученные данные в ASCII формате и hex формате
    let received_data = String::from_utf8_lossy(&buffer[..n]);
    error!("Received data (ASCII): {}", received_data);
    error!("Received data (HEX): {:?}", &buffer[..n].iter()
        .map(|b| format!("{:02x}", b))
        .collect::<Vec<String>>()
        .join(" "));
    error!("First 4 bytes: {:?}", &buffer[..4].iter()
        .map(|b| format!("{:02x}", b))
        .collect::<Vec<String>>()
        .join(" "));

    // Проверяем, начинается ли запрос с "GET "
    if !received_data.starts_with("GET ") {
        error!("initialization error: connection reset rmbt: {} {}", n, std::io::Error::last_os_error());
        return Ok(Stream::Plain(stream)); //todo error
    }

    // Преобразуем буфер в строку для поиска заголовков
    let request = String::from_utf8_lossy(&buffer[..n]);

    // Проверяем заголовки Upgrade через регулярные выражения
    let ws_regex = Regex::new(r"(?i)^upgrade:\s*websocket").unwrap();
    let rmbt_regex = Regex::new(r"(?i)^upgrade:\s*rmbt").unwrap();

    let is_websocket = ws_regex.is_match(&request);
    let is_rmbt = rmbt_regex.is_match(&request);

    if !is_websocket && !is_rmbt {
        info!("No HTTP upgrade to websocket/rmbt");
        stream.write_all(RMBT_UPGRADE.as_bytes()).await?;
        stream.flush().await?;
        return Ok(Stream::Plain(stream));
    }

    if is_rmbt {
        debug!("Upgrading to RMBT");
        // Отправляем HTTP upgrade ответ
        stream.write_all(RMBT_UPGRADE.as_bytes()).await?;
        stream.flush().await?;

        return if isSSL {
            match tls_acceptor {
                Some(acceptor) => {
                    debug!("Accepting TLS connection");
                    Ok(Stream::Tls(acceptor.accept(stream).await?))
                }
                None => {
                    error!("TLS acceptor is not available");
                    Err("Missing acceptor".into()) //todo error
                }
            }
        } else {
            Ok(Stream::Plain(stream))
        };
    }

    if is_websocket {
        debug!("Upgrading to WebSocket");

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

        // Convert to WebSocket stream
        return if isSSL {
            match tls_acceptor {
                Some(acceptor) => {
                    debug!("Accepting TLS connection");
                    let ws_stream = tokio_tungstenite::accept_async(acceptor.accept(stream).await?).await?;
                    Ok(Stream::WebSocketTls(ws_stream))
                }
                None => {
                    error!("TLS acceptor is not available");
                    Err("Missing acceptor".into()) //todo error
                }
            }
        } else {
            let ws_stream = tokio_tungstenite::accept_async(stream).await?;
            Ok(Stream::WebSocket(ws_stream))
        };
    }

    // Этот код никогда не должен выполниться, но компилятор требует возврата
    Ok(Stream::Plain(stream))
}

