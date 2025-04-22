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

    // Отправляем OK клиенту после проверки размера чанка
    stream.write_all(RESP_OK.as_bytes()).await?;
    stream.flush().await?;

    // Добавляем небольшую задержку перед началом чтения данных
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    // Начинаем измерение времени
    let start_time = Instant::now();
    let mut total_bytes = 0;
    let mut buffer = vec![0u8; chunk_size];
    let mut last_byte = 0u8;

    // Читаем данные до тех пор, пока не найдем терминатор (0xFF) или соединение не закроется
    loop {
        match stream.read(&mut buffer).await {
            Ok(n) => {
                info!("Received {} bytes PREUPLOAD", n);
                if n == 0 {
                    debug!("Connection closed by client during data transfer");
                    break;
                }

                total_bytes += n;
                debug!("Received {} bytes, total: {}", n, total_bytes);

                // Проверяем последний байт в полученных данных
                let last_pos = n - 1;
                last_byte = buffer[last_pos];
                debug!("Last byte at position {} is 0x{:02X}", last_pos, last_byte);
                if last_byte == 0xFF {
                    debug!("Found termination byte at last position {}", last_pos);
                    total_bytes -= (n - last_pos - 1); // Корректируем общее количество байт
                    break;
                }
            },
            Err(e) => {
                error!("Failed to read data: {}", e);
                // Не отправляем ошибку, так как клиент мог просто закрыть соединение
                break;
            }
        }
    }

    // Добавляем небольшую задержку перед отправкой TIME
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    // Отправляем финальный результат TIME
    let elapsed_ns = start_time.elapsed().as_nanos() as u64;
    let time_response = format!("{} {}\n", RESP_TIME, elapsed_ns);
    info!("Sending TIME response: {}", time_response);
    
    // Пробуем отправить TIME, но игнорируем ошибку, если соединение уже закрыто
    if let Err(e) = stream.write_all(time_response.as_bytes()).await {
        debug!("Failed to send TIME response: {}", e);
    }
    
    info!(
        "PUTNORESULT completed: received {} bytes in {} ns, found_terminator: {}",
        total_bytes,
        elapsed_ns,
        last_byte == 0xFF
    );

    Ok(())
} 