use std::error::Error;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_native_tls::TlsStream;
use log::{info, error};
use crate::protocol::parse_token;
use crate::utils::token_validator::TokenValidator;
use crate::utils::tls::TlsConfig;
use crate::handlers::*;

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

pub async fn handle_connection(
    stream: TcpStream,
    token_validator: Arc<TokenValidator>,
    tls_config: Option<Arc<TlsConfig>>,
    data_buffer: Arc<tokio::sync::Mutex<Vec<u8>>>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut stream = if let Some(tls_config) = tls_config {
        Stream::Tls(tls_config.accept(stream).await?)
    } else {
        Stream::Plain(stream)
    };

    crate::protocol::send_greeting(&mut stream, 3).await?;
    
    let token = crate::protocol::read_line(&mut stream).await?;
    let token = parse_token(&token)?;
    
    if !token_validator.validate(&token).await? {
        return Err("Invalid token".into());
    }
    
    crate::protocol::confirm_token_accepted(&mut stream).await?;
    command_loop(&mut stream, data_buffer).await
}

async fn command_loop(
    stream: &mut Stream,
    data_buffer: Arc<tokio::sync::Mutex<Vec<u8>>>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    loop {
        let line = crate::protocol::read_line(stream).await?;
        match line.as_str() {
            "QUIT" => {
                handle_quit(stream).await?;
                break;
            }
            "PING" => handle_ping(stream).await?,
            "GETTIME" => handle_get_time(stream).await?,
            "GETCHUNKS" => handle_get_chunks(stream, data_buffer.clone()).await?,
            "PUT" => handle_put(stream, data_buffer.clone()).await?,
            "PUTNORESULT" => handle_put_no_result(stream, data_buffer.clone()).await?,
            _ => return Err("Unknown command".into()),
        }
    }
    Ok(())
} 