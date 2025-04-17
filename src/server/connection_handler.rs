use crate::config::constants::{CHUNK_SIZE, ERR, MAX_CHUNKS, MAX_CHUNK_SIZE, OK, PONG, RESP_ERR, RESP_OK};
use crate::handlers::*;
use crate::handlers::{
    handle_get_chunks, handle_get_time, handle_ping, handle_put, handle_put_no_result, handle_quit,
};
use crate::protocol::*;
use crate::utils::token_validator::TokenValidator;
use log::{debug, error, info};
use std::error::Error;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Mutex;
use tokio_native_tls::TlsStream;
use tokio::net::TcpStream;
use crate::server::server_config::ServerConfig;

pub enum Stream {
    Plain(TcpStream),
    Tls(TlsStream<TcpStream>),
}

impl Stream {
    pub async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Box<dyn Error + Send + Sync>> {
        match self {
            Stream::Plain(stream) => Ok(stream.read(buf).await?),
            Stream::Tls(stream) => Ok(stream.read(buf).await?),
        }
    }

    pub async fn write(&mut self, buf: &[u8]) -> Result<usize, Box<dyn Error + Send + Sync>> {
        match self {
            Stream::Plain(stream) => Ok(stream.write(buf).await?),
            Stream::Tls(stream) => Ok(stream.write(buf).await?),
        }
    }

    pub async fn write_all(&mut self, buf: &[u8]) -> Result<(), Box<dyn Error + Send + Sync>> {
        match self {
            Stream::Plain(stream) => Ok(stream.write_all(buf).await?),
            Stream::Tls(stream) => Ok(stream.write_all(buf).await?),
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

        if let Err(e) = self.handle_token().await {
            eprintln!("Token validation failed: {}", e);
            return Err(e);
        }

        // Main command loop
        loop {
            let mut buffer = [0u8; 1024];
            match self.stream.read(&mut buffer).await {
                Ok(0) => break, // Connection closed
                Ok(n) => {
                    let command = String::from_utf8_lossy(&buffer[..n]);
                    match command.trim() {
                        "GETTIME" => handle_get_time(&mut self.stream).await?,
                        "GETCHUNKS" => {
                            handle_get_chunks(&mut self.stream, self.data_buffer.clone()).await?
                        }
                        "PUT" => handle_put(&mut self.stream, self.data_buffer.clone()).await?,
                        "PUTNORESULT" => {
                            handle_put_no_result(&mut self.stream, self.data_buffer.clone()).await?
                        }
                        "PING" => handle_ping(&mut self.stream).await?,
                        "QUIT" => {
                            handle_quit(&mut self.stream).await?;
                            break;
                        }
                        _ => {
                            self.stream.write_all(RESP_ERR.as_bytes()).await?;
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Read error: {}", e);
                    return Err(e.into());
                }
            }
        }

        Ok(())
    }

    async fn send_greeting(&mut self) -> Result<(), Box<dyn Error + Send + Sync>> {
        match self.config.version {
            Some(3) => {
                self.stream.write_all("RMBTv0.3\n".as_bytes()).await?;
            }
            None => {
                self.stream.write_all("RMBTv1.0\n".as_bytes()).await?;
            }
            _ => {
                // This should never happen as we validate version in Config::set_protocol_version
                self.stream.write_all("RMBTv1.0\n".as_bytes()).await?;
            }
        }
        self.stream
            .write_all("ACCEPT TOKEN QUIT\n".as_bytes())
            .await?;
        Ok(())
    }

    async fn handle_token(&mut self) -> Result<(), Box<dyn Error + Send + Sync>> {
        // Читаем строку с токеном
        let mut buffer = [0u8; 1024];
        let n = self.stream.read(&mut buffer).await?;
        if n == 0 {
            return Err("Connection closed before token received".into());
        }

        let token_line = String::from_utf8_lossy(&buffer[..n]);
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
