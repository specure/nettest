use std::error::Error;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_native_tls::TlsAcceptor;
use tokio_native_tls::TlsStream;
use crate::protocol::commands::{parse_command, Command};
use crate::config::constants::{RESP_OK, RESP_BYE, RESP_PONG};
use crate::handlers;
use crate::handlers::{handle_get_chunks, handle_get_time, handle_ping, handle_put, handle_put_no_result, handle_quit};

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
    tls_acceptor: Option<Arc<TlsAcceptor>>,
    data_buffer: Arc<tokio::sync::Mutex<Vec<u8>>>,
    use_ssl: bool,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut stream = if use_ssl {
        if let Some(acceptor) = tls_acceptor {
            Stream::Tls(acceptor.accept(stream).await?)
        } else {
            return Err("SSL requested but no TLS acceptor available".into());
        }
    } else {
        Stream::Plain(stream)
    };

    // Send greeting
    stream.write_all(RESP_OK.as_bytes()).await?;

    loop {
        let mut buf = [0u8; 1024];
        let n = stream.read(&mut buf).await?;
        if n == 0 {
            break;
        }

        let input = String::from_utf8_lossy(&buf[..n]);
        let command = parse_command(&input);

        match command {
            Command::Quit => {
                stream.write_all(RESP_BYE.as_bytes()).await?;
                break;
            }
            Command::Ping => {
                stream.write_all(RESP_PONG.as_bytes()).await?;
            }
            Command::GetTime => {
                handlers::handle_get_time(&mut stream).await?;
            }
            Command::GetChunks => {
                handlers::handle_get_chunks(&mut stream, data_buffer.clone()).await?;
            }
            Command::Put => {
                handlers::handle_put(&mut stream, data_buffer.clone()).await?;
            }
            Command::PutNoResult => {
                handlers::handle_put_no_result(&mut stream, data_buffer.clone()).await?;
            }
            Command::Unknown(cmd) => {
                stream.write_all(format!("ERR Unknown command: {}\r\n", cmd).as_bytes()).await?;
            }
        }
    }

    Ok(())
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