use std::time::Duration;

use futures::{SinkExt, StreamExt};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::sleep;
use tokio_native_tls::TlsStream;
use tokio_tungstenite::WebSocketStream;
use log::{info, error, debug};

const CHUNK_SIZE: usize = 4096;

#[derive(Debug)]
pub enum Stream {
    Plain(TcpStream),
    Tls(TlsStream<TcpStream>),
    WebSocket(WebSocketStream<TcpStream>),
    WebSocketTls(WebSocketStream<TlsStream<TcpStream>>),
}

impl Stream {
    pub async fn upgrade_to_websocket(self) -> std::io::Result<Stream> {
        match self {
            Stream::Plain(tcp_stream) => {
                info!("Attempting to upgrade plain TCP stream to WebSocket");
                let ws_stream = WebSocketStream::from_raw_socket(tcp_stream, tokio_tungstenite::tungstenite::protocol::Role::Server, None).await;
                info!("Successfully upgraded plain TCP stream to WebSocket");
                Ok(Stream::WebSocket(ws_stream))
            }
            Stream::Tls(tls_stream) => {
                info!("Attempting to upgrade TLS stream to WebSocket");
                let ws_stream = WebSocketStream::from_raw_socket(tls_stream, tokio_tungstenite::tungstenite::protocol::Role::Server, None).await;
                info!("Successfully upgraded TLS stream to WebSocket");
                Ok(Stream::WebSocketTls(ws_stream))
            }
            _ => {
                error!("Cannot upgrade non-TCP stream to WebSocket");
                Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "Cannot upgrade non-TCP stream to WebSocket"
                ))
            }
        }
    }

    pub async fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        // sleep(Duration::from_millis(50)).await;
        match self {
            Stream::Plain(stream) => stream.read(buf).await,
            Stream::Tls(stream) => stream.read(buf).await,
            Stream::WebSocket(stream) => {
                if let Some(msg) = stream.next().await {
                    match msg {
                        Ok(msg) => {
                            let data = match msg {
                                tokio_tungstenite::tungstenite::Message::Binary(data) => data,
                                tokio_tungstenite::tungstenite::Message::Text(text) => text.into_bytes(),
                                _ => return Ok(0),
                            };
                            let len = data.len().min(buf.len());
                            buf[..len].copy_from_slice(&data[..len]);
                            Ok(len)
                        }
                        Err(e) => Err(std::io::Error::new(std::io::ErrorKind::Other, e)),
                    }
                } else {
                    Ok(0)
                }
            }
            Stream::WebSocketTls(stream) => {
                if let Some(msg) = stream.next().await {
                    match msg {
                        Ok(msg) => {
                            let data = match msg {
                                tokio_tungstenite::tungstenite::Message::Binary(data) => data,
                                tokio_tungstenite::tungstenite::Message::Text(text) => text.into_bytes(),
                                _ => return Ok(0),
                            };
                            let len = data.len().min(buf.len());
                            buf[..len].copy_from_slice(&data[..len]);
                            Ok(len)
                        }
                        Err(e) => Err(std::io::Error::new(std::io::ErrorKind::Other, e)),
                    }
                } else {
                    Ok(0)
                }
            }
        }
    }

    pub async fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Stream::Plain(stream) => stream.write(buf).await,
            Stream::Tls(stream) => stream.write(buf).await,
            Stream::WebSocket(stream) => {
                debug!("WebSocket: Preparing to send message in write, buf len: {}", buf.len());
                let message = if buf.len() < 2 || buf.len() > (CHUNK_SIZE - 3) {
                    debug!("WebSocket: Sending binary message in write");
                    tokio_tungstenite::tungstenite::Message::Binary(buf.to_vec())
                } else {
                    debug!("WebSocket: Sending text message in write");
                    tokio_tungstenite::tungstenite::Message::Text(String::from_utf8_lossy(buf).to_string())
                };
                debug!("WebSocket: Before stream.send in write");
                stream.send(message).await
                    .map_err(|e| {
                        error!("WebSocket: Error sending message in write: {}", e);
                        std::io::Error::new(std::io::ErrorKind::Other, e)
                    })?;
                debug!("WebSocket: After stream.send in write");
                Ok(buf.len())
            }
            Stream::WebSocketTls(stream) => {
                debug!("WebSocketTls: Preparing to send message in write, buf len: {}", buf.len());
                let message = if buf.len() < 2 || buf.len() > (CHUNK_SIZE - 3) {
                    debug!("WebSocketTls: Sending binary message in write");
                    tokio_tungstenite::tungstenite::Message::Binary(buf.to_vec())
                } else {
                    debug!("WebSocketTls: Sending text message in write");
                    tokio_tungstenite::tungstenite::Message::Text(String::from_utf8_lossy(buf).to_string())
                };
                debug!("WebSocketTls: Before stream.send in write");
                stream.send(message).await
                    .map_err(|e| {
                        error!("WebSocketTls: Error sending message in write: {}", e);
                        std::io::Error::new(std::io::ErrorKind::Other, e)
                    })?;
                debug!("WebSocketTls: After stream.send in write");
                Ok(buf.len())
            }
        }
    }

    pub async fn write_all(&mut self, buf: &[u8]) -> std::io::Result<()> {

        match self {
            Stream::Plain(stream) => stream.write_all(buf).await,
            Stream::Tls(stream) => stream.write_all(buf).await,
            Stream::WebSocket(stream) => {
                debug!("WebSocket: Preparing to send message, buf len: {}", buf.len());
                let message = if buf.len() < 2 || buf.len() > (CHUNK_SIZE - 3) {
                    debug!("WebSocket: Sending binary message");
                    tokio_tungstenite::tungstenite::Message::Binary(buf.to_vec())
                } else {
                    debug!("WebSocket: Sending text message");
                    tokio_tungstenite::tungstenite::Message::Text(String::from_utf8_lossy(buf).to_string())
                };
                debug!("WebSocket: Before stream.send");
                stream.send(message).await
                    .map_err(|e| {
                        error!("WebSocket: Error sending message: {}", e);
                        std::io::Error::new(std::io::ErrorKind::Other, e)
                    })?;
                debug!("WebSocket: After stream.send");
                Ok(())
            }
            Stream::WebSocketTls(stream) => {
                debug!("WebSocketTls: Preparing to send message, buf len: {}", buf.len());
                let message = if buf.len() < 2 || buf.len() > (CHUNK_SIZE - 3) {
                    debug!("WebSocketTls: Sending binary message");
                    tokio_tungstenite::tungstenite::Message::Binary(buf.to_vec())
                } else {
                    debug!("WebSocketTls: Sending text message");
                    tokio_tungstenite::tungstenite::Message::Text(String::from_utf8_lossy(buf).to_string())
                };
                debug!("WebSocketTls: Before stream.send");
                stream.send(message).await
                    .map_err(|e| {
                        error!("WebSocketTls: Error sending message: {}", e);
                        std::io::Error::new(std::io::ErrorKind::Other, e)
                    })?;
                debug!("WebSocketTls: After stream.send");
                Ok(())
            }
        }
    }

    pub async fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Stream::Plain(stream) => stream.flush().await,
            Stream::Tls(stream) => stream.flush().await,
            Stream::WebSocket(_) => Ok(()),
            Stream::WebSocketTls(_) => Ok(()),
        }
    }

    pub fn to_string(&self) -> String {
        match self {
            Stream::Plain(_) => "Plain".to_string(),
            Stream::Tls(_) => "TLS".to_string(),
            Stream::WebSocket(_) => "WebSocket".to_string(),
            Stream::WebSocketTls(_) => "WebSocketTLS".to_string(),
        }
    }
}
    