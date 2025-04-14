use std::error::Error;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use crate::config::constants::*;

#[derive(Debug)]
pub enum Command {
    GetChunks,
    GetTime,
    Put,
    PutNoResult,
    Ping,
    Quit,
    Unknown(String),
}

pub fn parse_command(input: &str) -> Command {
    let input = input.trim();
    match input {
        s if s == CMD_GETCHUNKS => Command::GetChunks,
        s if s == CMD_GETTIME => Command::GetTime,
        s if s == CMD_PUT => Command::Put,
        s if s == CMD_PUTNORESULT => Command::PutNoResult,
        s if s == CMD_PING => Command::Ping,
        s if s == CMD_QUIT => Command::Quit,
        s => Command::Unknown(s.to_string()),
    }
}

pub async fn send_response(socket: &mut TcpStream, response: &str) -> Result<(), Box<dyn Error + Send + Sync>> {
    socket.write_all(response.as_bytes()).await?;
    Ok(())
} 