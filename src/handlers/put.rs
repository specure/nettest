use std::time::Instant;
use tokio::io::{AsyncReadExt, Interest};
use log::{info, debug, error};
use crate::server::connection_handler::Stream;
use crate::config::constants::{CHUNK_SIZE, MIN_CHUNK_SIZE, MAX_CHUNK_SIZE, RESP_OK, RESP_ERR, RESP_TIME};
use std::net::TcpStream;
// use std::net::Interest;

pub async fn handle_put(
    stream: &mut Stream,
    command: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Парсим команду: PUT [chunk_size]
    let parts: Vec<&str> = command.split_whitespace().collect();

    // Определяем размер чанка
    let chunk_size = if parts.len() == 2 {
        // Если указан размер чанка, проверяем его
        match parts[1].parse::<usize>() {
            Ok(size) if size >= MIN_CHUNK_SIZE && size <= MAX_CHUNK_SIZE => size,
            _ => {
                stream.write_all(RESP_ERR.as_bytes()).await?;
                return Ok(());
            }
        }
    } else {
        CHUNK_SIZE
    };

    info!("Starting PUT process: chunk_size={}", chunk_size);

    // Отправляем OK клиенту
    stream.write_all(RESP_OK.as_bytes()).await?;

    // Начинаем измерение времени
    let start_time = Instant::now();
    let mut total_bytes = 0;
    let mut buffer = vec![0u8; chunk_size];
    let mut last_byte = 0;
    let mut last_time_ns: i64 = -1;
    // Читаем данные до тех пор, пока получаем данные и не нашли терминатор
    loop {
        match stream.read(&mut buffer).await {
            Ok(n) => {
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
                    break;
                }

                // Отправляем TIME только если прошло больше 1мс с последней отправки
                let current_time_ns = start_time.elapsed().as_nanos() as i64;
                if last_time_ns == -1 || (current_time_ns - last_time_ns > 1_000_000) {
                    last_time_ns = current_time_ns;
                    let time_response = format!("{} {} BYTES {}\n", RESP_TIME, current_time_ns, total_bytes);
                    info!("Sending TIME response: {}", time_response);
                    if let Err(e) = stream.write_all(time_response.as_bytes()).await {
                        debug!("Failed to send TIME response: {}", e);
                        break;
                    }
                }
            },

            Err(e) => {
                error!("Failed to read data: {}", e);
                // Не отправляем ошибку, так как клиент мог просто закрыть соединение
                break;
            }
        }
    }

    // Отправляем финальный TIME с BYTES
    let elapsed_ns = start_time.elapsed().as_nanos() as u64;
    let time_response = format!("{} {} BYTES {}\n", RESP_TIME, elapsed_ns, total_bytes);
    info!("Sending final TIME response: {}", time_response);
    
    if let Err(e) = stream.write_all(time_response.as_bytes()).await {
        debug!("Failed to send final TIME response: {}", e);
    }
    //
    // // Отправляем OK только после получения всех данных и терминатора
    // stream.write_all(RESP_OK.as_bytes()).await?;
    
    info!(
        "PUT completed: received {} bytes in {} ns, last_byte: 0x{:02X}, chunk_size: {}",
        total_bytes,
        elapsed_ns,
        last_byte,
        chunk_size
    );

    Ok(())
}
