use crate::config::constants::{ACCEPT_COMMANDS, CHUNK_SIZE, ERR, MAX_CHUNKS, MAX_CHUNK_SIZE, MIN_CHUNK_SIZE, OK, PONG, RESP_ERR, RESP_OK};
use crate::handlers::*;
use crate::handlers::{
    handle_get_chunks, handle_get_time, handle_ping, handle_put, handle_put_no_result, handle_quit,
};
use crate::utils::token_validator::TokenValidator;
use log::{debug, error, info};
use std::error::Error;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Mutex;
use tokio_native_tls::TlsStream;
use tokio::net::TcpStream;
use tokio_tungstenite::WebSocketStream;
use futures::{SinkExt, StreamExt};
use crate::server::server_config::ServerConfig;
use std::net::SocketAddr;

#[derive(Debug)]
pub enum Stream {
    Plain(TcpStream),
    Tls(TlsStream<TcpStream>),
    WebSocket(WebSocketStream<TcpStream>),
    WebSocketTls(WebSocketStream<TlsStream<TcpStream>>),
}

impl Stream {
    pub async fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Stream::Plain(stream) => stream.read(buf).await,
            Stream::Tls(stream) => stream.read(buf).await,
            Stream::WebSocket(stream) => {
                if let Some(msg) = stream.next().await {
                    match msg {
                        Ok(msg) => {
                            let data = msg.into_data();
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
                            let data = msg.into_data();
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
                stream.send(tokio_tungstenite::tungstenite::Message::Binary(buf.to_vec())).await
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
                Ok(buf.len())
            }
            Stream::WebSocketTls(stream) => {
                stream.send(tokio_tungstenite::tungstenite::Message::Binary(buf.to_vec())).await
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
                Ok(buf.len())
            }
        }
    }

    pub async fn write_all(&mut self, buf: &[u8]) -> std::io::Result<()> {
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

pub struct ConnectionHandler {
    stream: Stream,
    config: Arc<ServerConfig>,
    token_validator: Arc<TokenValidator>,
    data_buffer: Arc<Mutex<Vec<u8>>>,
}

impl ConnectionHandler {
    pub fn new(
        stream: Stream,
        config: Arc<ServerConfig>,
        token_validator: Arc<TokenValidator>,
        data_buffer: Arc<Mutex<Vec<u8>>>,
    ) -> Self {
        Self {
            stream,
            config,
            token_validator,
            data_buffer,
        }
    }

    pub async fn handle(&mut self) -> Result<(), Box<dyn Error + Send + Sync>> {
        if let Err(e) = self.send_greeting().await {
            eprintln!("Failed to send greeting: {}", e);
            return Err(e);
        }

        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;


        if let Err(e) = self.handle_token().await {
            eprintln!("Token validation failed: {}", e);
            return Err(e);
        }

        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        // Отправляем информацию о размерах чанков
        let chunk_size_msg = format!("CHUNKSIZE {} {} {}\n", CHUNK_SIZE, MIN_CHUNK_SIZE, MAX_CHUNK_SIZE); //todo compare version
        self.stream.write_all(chunk_size_msg.as_bytes()).await?;

        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        self.stream.write_all(ACCEPT_COMMANDS.as_bytes()).await?;
        // Main command loop
        loop {
            let mut buffer = [0u8; 1024];

            match self.stream.read(&mut buffer).await {
                Ok(0) => break, // Connection closed
                Ok(n) => {
                    let command = String::from_utf8_lossy(&buffer[..n]);
                    // Берем только первую строку как команду
                    let command_str = command.lines().next().unwrap_or("").trim();

                    // Пропускаем бинарные данные, которые не похожи на команду
                    if command_str.is_empty() || !command_str.chars().all(|c| c.is_ascii_alphanumeric() || c.is_ascii_whitespace() || c == '.') {
                        debug!("Received binary data, sending OK");
                        self.stream.write_all(RESP_OK.as_bytes()).await?;
                        continue;
                    }

                    println!("Received command: {}", command_str);

                    // Если это PUT или PUTNORESULT, передаем все прочитанные данные в обработчик
                    if command_str.starts_with("PUT") || command_str.starts_with("PUTNORESULT") {
                        if command_str.starts_with("PUTNORESULT") {
                            handle_put_no_result(&mut self.stream, command_str).await?;
                        } else {
                            handle_put(&mut self.stream, command_str).await?;
                        }
                    } else if command_str.starts_with("GETTIME") {
                        println!("Handling GETTIME command");
                        handle_get_time(&mut self.stream, command_str).await?
                    } else if command_str.starts_with("GETCHUNKS") {
                        handle_get_chunks(&mut self.stream, command_str).await?
                    } else if command_str == "PING" {
                        handle_ping(&mut self.stream).await?
                    } else if command_str == "QUIT" {
                        handle_quit(&mut self.stream).await?;
                        break;
                    } else {
                        debug!("Unknown command: {}", command_str);
                        self.stream.write_all(RESP_ERR.as_bytes()).await?;
                    }
                }
                Err(e) => {
                    eprintln!("Read error: {}", e);
                    return Err(e.into());
                }
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;

            self.stream.write_all(ACCEPT_COMMANDS.as_bytes()).await?;
        }

        Ok(())
    }

    async fn send_greeting(&mut self) -> Result<(), Box<dyn Error + Send + Sync>> {
        // Отправляем приветствие
        let greeting = match self.config.version {
            Some(3) => "RMBTv0.3\n",
            None => "RMBTv1.0\n",
            _ => "RMBTv1.0\n",
        };

        info!("Sending greeting message: {}", greeting);
        let written = self.stream.write(greeting.as_bytes()).await?;
        info!("Written {} bytes for greeting", written);
        self.stream.flush().await?;
        info!("Greeting message sent and flushed");

        // Добавляем небольшую задержку
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        // Отправляем ACCEPT TOKEN QUIT отдельным сообщением
        let accept_token = "ACCEPT TOKEN QUIT";
        info!("Sending accept token message: {}", accept_token);
        let written = self.stream.write(accept_token.as_bytes()).await?;
        info!("Written {} bytes for accept token", written);
        self.stream.flush().await?;
        info!("Accept token message sent and flushed");

        Ok(())
    }

    async fn handle_token(&mut self) -> Result<(), Box<dyn Error + Send + Sync>> {
        // Читаем строку с токеном
        info!("Waiting for token...");
        let mut buffer = [0u8; 1024];
        let n = self.stream.read(&mut buffer).await?;
        if n == 0 {
            return Err("Connection closed before token received".into());
        }

        let token_line = String::from_utf8_lossy(&buffer[..n]);
        debug!("Received token line: {}", token_line.to_string());

        let token_line = token_line.trim();

        // Проверяем формат строки TOKEN uuid_starttime_hmac
        if !token_line.starts_with("TOKEN ") {
            return Err("Invalid token format: must start with 'TOKEN '".into());
        }

        let token_parts: Vec<&str> = token_line[6..].split('_').collect();
        if token_parts.len() != 3 {
            return Err("Invalid token format: must be TOKEN uuid_starttime_hmac".into());
        }

        let uuid = token_parts[0];
        let start_time = token_parts[1];
        let hmac = token_parts[2];

        // Проверяем формат UUID (36 символов, только hex и дефисы)
        if uuid.len() != 36 || !uuid.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
            return Err("Invalid UUID format".into());
        }

        // Проверяем формат времени (только цифры)
        if !start_time.chars().all(|c| c.is_ascii_digit()) {
            return Err("Invalid time format".into());
        }

        // Проверяем формат HMAC (base64)
        if !hmac.chars().all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=') {
            return Err("Invalid HMAC format".into());
        }

        if self.token_validator.validate(uuid, start_time, hmac).await? {
            info!("Valid token; uuid: {}", uuid);
            self.stream.write_all("OK\n".as_bytes()).await?;
            Ok(())
        } else {
            error!("Token was not accepted");
            self.stream.write_all("ERR\n".as_bytes()).await?;
            Err("Invalid token".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::SocketAddr;
    use std::str::FromStr;
    use tokio::net::TcpListener;
    use tokio::net::TcpStream;

    #[tokio::test]
    async fn test_handle_connection() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        // Client connection
        let client = TcpStream::connect(addr).await.unwrap();

        // Server accepts connection
        let (server, _) = listener.accept().await.unwrap();

        let config = Arc::new(ServerConfig::default());
        let token_validator = Arc::new(TokenValidator::new(vec![], vec![]));
        let data_buffer = Arc::new(Mutex::new(Vec::new()));

        let mut handler = ConnectionHandler::new(
            Stream::Plain(server),
            config,
            token_validator,
            data_buffer,
        );

        // Test connection handling
        tokio::spawn(async move {
            handler.handle().await.unwrap();
        });

        // Clean up
        drop(client);
    }

    #[tokio::test]
    async fn test_handle_quit() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        // Client connection
        let client = TcpStream::connect(addr).await.unwrap();

        // Server accepts connection
        let (server, _) = listener.accept().await.unwrap();

        let config = Arc::new(ServerConfig::default());
        let token_validator = Arc::new(TokenValidator::new(vec![], vec![]));
        let data_buffer = Arc::new(Mutex::new(Vec::new()));

        let mut handler = ConnectionHandler::new(
            Stream::Plain(server),
            config,
            token_validator,
            data_buffer,
        );

        // Test quit command
        tokio::spawn(async move {
            handler.handle().await.unwrap();
        });

        // Clean up
        drop(client);
    }

    #[tokio::test]
    async fn test_handle_ping() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        // Client connection
        let client = TcpStream::connect(addr).await.unwrap();

        // Server accepts connection
        let (server, _) = listener.accept().await.unwrap();

        let config = Arc::new(ServerConfig::default());
        let token_validator = Arc::new(TokenValidator::new(vec![], vec![]));
        let data_buffer = Arc::new(Mutex::new(Vec::new()));

        let mut handler = ConnectionHandler::new(
            Stream::Plain(server),
            config,
            token_validator,
            data_buffer,
        );

        // Test ping command
        tokio::spawn(async move {
            handler.handle().await.unwrap();
        });

        // Clean up
        drop(client);
    }
}
