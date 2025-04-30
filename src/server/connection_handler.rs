use crate::config::constants::{ACCEPT_COMMANDS, CHUNK_SIZE, MAX_CHUNK_SIZE, MIN_CHUNK_SIZE, RESP_ERR};
use crate::handlers::{
    handle_get_chunks, handle_get_time, handle_ping, handle_put, handle_put_no_result, handle_quit,
};
use crate::utils::token_validator::TokenValidator;
use log::{debug, error, info};
use std::error::Error;
use std::sync::Arc;
use crate::server::server_config::ServerConfig;
use crate::stream::Stream;

pub struct ConnectionHandler {
    stream: Stream,
    config: Arc<ServerConfig>,
    token_validator: Arc<TokenValidator>,
}

impl ConnectionHandler {
    pub fn new(
        stream: Stream,
        config: Arc<ServerConfig>,
        token_validator: Arc<TokenValidator>,
    ) -> Self {
        Self {
            stream,
            config,
            token_validator,
        }
    }

    pub async fn handle(&mut self) -> Result<(), Box<dyn Error + Send + Sync>> {
        info!("Handling connection");
        if let Err(e) = self.send_greeting().await {
            error!("Failed to send greeting: {}", e);
            return Err(e);
        }
        info!("Greeting sent");


        if let Err(e) = self.handle_token().await {
            error!("Token validation failed: {}", e);
            return Err(e);
        }
        info!("Token validated");

        let chunk_size_msg = format!("CHUNKSIZE {} {} {}\n", CHUNK_SIZE, MIN_CHUNK_SIZE, MAX_CHUNK_SIZE); //todo compare version
        self.stream.write_all(chunk_size_msg.as_bytes()).await?;
        self.stream.flush().await?;


        // Main command loop
        loop {
            self.stream.write_all(ACCEPT_COMMANDS.as_bytes()).await?;

            let mut buffer = [0u8; 1024];

            match self.stream.read(&mut buffer).await {
                Ok(0) => {
                    debug!("Client closed connection gracefully");
                    break;
                }
                Ok(n) => {
                    let command = String::from_utf8_lossy(&buffer[..n]);
                    let command_str = command.lines().next().unwrap_or("").trim();

                    debug!("Received command: {}", command_str);

                    if command_str.starts_with("PUT") || command_str.starts_with("PUTNORESULT") {
                        if command_str.starts_with("PUTNORESULT") {
                            handle_put_no_result(&mut self.stream, command_str).await?;
                        } else {
                            handle_put(&mut self.stream, command_str).await?;
                        }
                    } else if command_str.starts_with("GETTIME") {
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
                        self.stream.write_all((RESP_ERR.to_owned() + command_str).as_bytes()).await?;
                    }
                }
                Err(e) => {
                    match e.kind() {
                        std::io::ErrorKind::ConnectionReset => {
                            debug!("Client reset connection");
                            break;
                        }
                        std::io::ErrorKind::ConnectionAborted => {
                            debug!("Client aborted connection");
                            break;
                        }
                        std::io::ErrorKind::BrokenPipe => {
                            debug!("Broken pipe - client closed connection");
                            break;
                        }
                        _ => {
                            error!("Error reading from stream: {}", e);
                            return Err(e.into());
                        }
                    }
                }
            }

        }

        Ok(())
    }

    async fn send_greeting(&mut self) -> Result<(), Box<dyn Error + Send + Sync>> {
        let greeting = match self.config.version {
            Some(3) => "RMBTv0.3\n",
            None => "RMBTv1.0\n",
            _ => "RMBTv1.3.3\n",
        };  //TODO

        debug!("Sending greeting message: {}", greeting);
        let written = self.stream.write(greeting.as_bytes()).await?;
        debug!("Written {} bytes for greeting", written);
        self.stream.flush().await?;
        debug!("Greeting message sent and flushed");

        let accept_token = "ACCEPT TOKEN QUIT\n";
        debug!("Sending accept token message: {}", accept_token);
        let written = self.stream.write(accept_token.as_bytes()).await?;
        debug!("Written {} bytes for accept token", written);
        self.stream.flush().await?;
        debug!("Accept token message sent and flushed");

        Ok(())
    }

    async fn handle_token(&mut self) -> Result<(), Box<dyn Error + Send + Sync>> {
        // Read token line
        debug!("Waiting for token...");
        let mut buffer = [0u8; 1024];
        let n = self.stream.read(&mut buffer).await?;
        if n == 0 {
            return Err("Connection closed before token received".into());
        }

        let token_line = String::from_utf8_lossy(&buffer[..n]);
        debug!("Received token line: {}", token_line.to_string());

        let token_line = token_line.trim();

        // Check token format: TOKEN uuid_starttime_hmac
        if !token_line.starts_with("TOKEN ") {
            error!("{}", token_line.to_string());
            return Err("Invalid token format: must start with 'TOKEN '".into());
        }

        let token_parts: Vec<&str> = token_line[6..].split('_').collect();
        if token_parts.len() != 3 {
            return Err("Invalid token format: must be TOKEN uuid_starttime_hmac".into());
        }

        let uuid = token_parts[0];
        let start_time = token_parts[1];
        let hmac = token_parts[2];

        // Check UUID format (36 characters, only hex and dashes)
        if uuid.len() != 36 || !uuid.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
            return Err("Invalid UUID format".into());
        }

        // Check time format (only digits)
        if !start_time.chars().all(|c| c.is_ascii_digit()) {
            return Err("Invalid time format".into());
        }

        // Check HMAC format (base64)
        if !hmac.chars().all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=') {
            return Err("Invalid HMAC format".into());
        }

        if self.token_validator.validate(uuid, start_time, hmac).await? {
            info!("Valid token; uuid: {}", uuid);
            self.stream.write_all("OK\n".as_bytes()).await?;
            Ok(())
        } else {
            error!("Token was not accepted");
            self.stream.write_all(RESP_ERR.as_bytes()).await?;
            Err("Invalid token".into())
        }
    }

}

#[cfg(test)]
mod tests {
    use super::*;
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

        let mut handler = ConnectionHandler::new(
            Stream::Plain(server),
            config,
            token_validator,
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

        let mut handler = ConnectionHandler::new(
            Stream::Plain(server),
            config,
            token_validator,
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

        let mut handler = ConnectionHandler::new(
            Stream::Plain(server),
            config,
            token_validator,
        );

        // Test ping command
        tokio::spawn(async move {
            handler.handle().await.unwrap();
        });

        // Clean up
        drop(client);
    }
}
