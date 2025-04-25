use futures::{SinkExt, StreamExt};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_native_tls::TlsStream;
use tokio_tungstenite::WebSocketStream;

#[derive(Debug)]
pub enum Stream {
    Plain(TcpStream),
    Tls(TlsStream<TcpStream>),
    WebSocket(WebSocketStream<TcpStream>),
    WebSocketTls(WebSocketStream<TlsStream<TcpStream>>),
}

impl Stream {
    pub async fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {

        // Добавляем небольшую задержку перед отправкой TIME
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
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
        //TODO test parameter
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        match self {
            Stream::Plain(stream) => stream.write(buf).await,
            Stream::Tls(stream) => stream.write(buf).await,
            Stream::WebSocket(stream) => {
                let message = tokio_tungstenite::tungstenite::Message::Binary(buf.to_vec());
                stream.send(message).await
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
                Ok(buf.len())
            }
            Stream::WebSocketTls(stream) => {
                let message = tokio_tungstenite::tungstenite::Message::Binary(buf.to_vec());
                stream.send(message).await
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
                Ok(buf.len())
            }
        }
    }

    pub async fn write_all(&mut self, buf: &[u8]) -> std::io::Result<()> {
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        match self {
            Stream::Plain(stream) => stream.write_all(buf).await,
            Stream::Tls(stream) => stream.write_all(buf).await,
            Stream::WebSocket(stream) => {
                stream.send(tokio_tungstenite::tungstenite::Message::Binary(buf.to_vec())).await
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
                Ok(())
            }
            Stream::WebSocketTls(stream) => {
                stream.send(tokio_tungstenite::tungstenite::Message::Binary(buf.to_vec())).await
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
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