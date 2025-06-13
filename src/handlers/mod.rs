use crate::config::constants::{RESP_BYE};
use crate::stream::Stream;
use log::{info};
use std::error::Error;

mod get_chunks;
mod get_time;
mod put;
mod put_no_result;
mod ping;

pub async fn handle_get_time(
    stream: &mut Stream,
    command: &str,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    get_time::handle_get_time(stream, command).await
}

pub async fn handle_get_chunks(
    stream: &mut Stream,
    command: &str,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    get_chunks::handle_get_chunks(stream, command).await
}

pub async fn handle_put(
    stream: &mut Stream,
    command: &str,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    put::handle_put(stream, command).await
}


pub async fn handle_ping(
    stream: &mut Stream,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    ping::handle_ping(stream).await
}

pub async fn handle_put_no_result(
    stream: &mut Stream,
    command: &str,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    put_no_result::handle_put_no_result(stream, command).await
}

pub async fn handle_quit(stream: &mut Stream) -> Result<(), Box<dyn Error + Send + Sync>> {
    // Send BYE to client
    stream.write_all(RESP_BYE.as_bytes()).await?;

    Ok(())
}

pub fn is_command(data: &[u8]) -> Option<String> {
    if let Ok(command_str) = String::from_utf8(data.to_vec()) {
        let trimmed = command_str.trim();
        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        if parts.len() >= 3 {
            return Some(trimmed.to_string());
        }
    }
    None
}
