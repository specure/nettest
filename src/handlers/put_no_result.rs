use std::time::Instant;
use tokio::io::AsyncReadExt;
use log::{info, debug, error};
use crate::server::connection_handler::Stream;
use crate::config::constants::{CHUNK_SIZE, MIN_CHUNK_SIZE, MAX_CHUNK_SIZE, RESP_OK, RESP_ERR, RESP_TIME};

pub async fn handle_put_no_result(
    stream: &mut Stream,
    command: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Парсим команду: PUTNORESULT [chunk_size]
    let parts: Vec<&str> = command.split_whitespace().collect();
    if parts.len() > 2 {
        stream.write_all(RESP_ERR.as_bytes()).await?;
        return Err("Invalid number of arguments for PUTNORESULT".into());
    }

    // Определяем размер чанка
    let chunk_size = if parts.len() == 2 {
        // Если указан размер чанка, проверяем его
        match parts[1].parse::<usize>() {
            Ok(size) if size >= MIN_CHUNK_SIZE && size <= MAX_CHUNK_SIZE => size,
            _ => {
                stream.write_all(RESP_ERR.as_bytes()).await?;
                return Err("Invalid chunk size".into());
            }
        }
    } else {
        CHUNK_SIZE
    };
    
    info!("Starting PUTNORESULT process: chunk_size={}", chunk_size);

    // Отправляем OK клиенту
    stream.write_all(RESP_OK.as_bytes()).await?;
    stream.flush().await?;

    // Начинаем измерение времени
    let start_time = Instant::now();
    let mut total_bytes = 0;
    let mut buffer = vec![0u8; chunk_size];
    let mut found_terminator = false;

    // Читаем данные до тех пор, пока не найдем терминатор (0xFF)
    while !found_terminator {
        match stream.read(&mut buffer).await {
            Ok(n) => {
                if n == 0 {
                    debug!("Connection closed by client during data transfer");
                    break;
                }

                total_bytes += n;
                debug!("Received {} bytes, total: {}", n, total_bytes);

                // Проверяем последний байт
                if n > 0 && buffer[n - 1] == 0xFF {
                    debug!("Found termination byte in last position");
                    found_terminator = true;
                }
            },
            Err(e) => {
                error!("Failed to read data: {}", e);
                stream.write_all(RESP_ERR.as_bytes()).await?;
                return Err(e.into());
            }
        }
    }

    // Отправляем финальный результат TIME
    let elapsed_ns = start_time.elapsed().as_nanos() as u64;
    let time_response = format!("{} {}\n", RESP_TIME, elapsed_ns);
    stream.write_all(time_response.as_bytes()).await?;
    stream.flush().await?;

    info!(
        "PUTNORESULT completed: received {} bytes in {} ns",
        total_bytes,
        elapsed_ns
    );

    Ok(())
} 