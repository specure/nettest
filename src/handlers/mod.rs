use crate::config::constants::{RESP_BYE, RESP_ERR, RESP_PONG, RESP_TIME};
use crate::stream::Stream;
use log::{error, info};
use std::error::Error;
use std::time::Instant;

mod get_chunks;
mod get_time;
mod put;
mod put_no_result;

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

pub async fn handle_put_no_result(
    stream: &mut Stream,
    command: &str,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    put_no_result::handle_put_no_result(stream, command).await
}

pub async fn handle_ping(stream: &mut Stream) -> Result<(), Box<dyn Error + Send + Sync>> {
    // Начало измерения времени
    let start_time = Instant::now();

    // Отправляем PONG клиенту
    stream.write_all(RESP_PONG.as_bytes()).await?;
    stream.flush().await?;

    info!("Send PONG 2");

    // Читаем ответ OK от клиента
    let mut buf = [0u8; 1024]; // Фиксированный размер буфера как в C-коде
    let mut bytes_read = 0;

    while bytes_read < buf.len() {
        match stream.read(&mut buf[bytes_read..]).await? {
            0 => break, // EOF
            n => {
                bytes_read += n;
                if let Some(pos) = buf[..bytes_read].iter().position(|&b| b == b'\n') {
                    bytes_read = pos + 1;
                    break;
                }
            }
        }
    }

    let response = String::from_utf8_lossy(&buf[..bytes_read])
        .trim()
        .to_string();

    // Конец измерения времени
    let elapsed_ns = start_time.elapsed().as_nanos() as u64;

    info!("Probably Received OK from client");
    
    // Проверяем ответ клиента
    if !response.trim().eq("OK") {
        error!("Expected OK from client, got: {}", response);
        stream.write_all(RESP_ERR.as_bytes()).await?;
        return Err("Invalid client response".into());
    } else {
        info!("Received OK from client");
    }

    info!("Send PING response time: {} ns", elapsed_ns);
    // Отправляем время выполнения
    let time_response = format!("{} {}\n", RESP_TIME, elapsed_ns);
    stream.write_all(time_response.as_bytes()).await?;
    stream.flush().await?;

    info!("PING completed in {} ns", elapsed_ns);

    Ok(())
}

pub async fn handle_quit(stream: &mut Stream) -> Result<(), Box<dyn Error + Send + Sync>> {
    // Отправляем BYE клиенту
    stream.write_all(RESP_BYE.as_bytes()).await?;
    stream.flush().await?;

    info!("Client requested QUIT, connection will be closed");

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
