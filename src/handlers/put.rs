use std::time::Instant;
use tokio::io::AsyncReadExt;
use log::{info, debug, error};
use crate::server::connection_handler::Stream;
use crate::config::constants::{CHUNK_SIZE, MIN_CHUNK_SIZE, MAX_CHUNK_SIZE, RESP_OK, RESP_ERR, RESP_TIME};

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

    // Читаем данные до тех пор, пока получаем данные и не нашли терминатор
    loop {
        match stream.read(&mut buffer).await {
            Ok(n) => {
                if n == 0 {
                    debug!("Connection closed by client during data transfer");
                    break;
                }

                // Обрабатываем бинарные данные
                info!("Received {} bytes", n);
                total_bytes += n;
                debug!("Received {} bytes, total: {}", n, total_bytes);

                // Вычисляем позицию последнего байта в текущем чанке
                let pos_last = chunk_size - 1 - (total_bytes % chunk_size);
                debug!("Last byte position in chunk: {}", pos_last);
                
                // Проверяем последний байт только если прочитали достаточно данных
                if n > pos_last {
                    last_byte = buffer[pos_last];
                    debug!("Last byte at position {} is 0x{:02X}", pos_last, last_byte);
                    if last_byte == 0xFF {
                        debug!("Found termination byte at position {}", pos_last);
                        total_bytes -= 1; // Исключаем терминатор из общего количества
                        break;
                    }
                }

                // Отправляем TIME после каждого чанка данных
                let elapsed_ns = start_time.elapsed().as_nanos() as u64;
                let time_response = format!("{} {} BYTES {}\n", RESP_TIME, elapsed_ns, total_bytes);
                info!("Sending TIME response: {}", time_response);
                if let Err(e) = stream.write_all(time_response.as_bytes()).await {
                    debug!("Failed to send TIME response: {}", e);
                    break;
                }

                // Если получили все данные, выходим из цикла
                if total_bytes >= chunk_size {
                    debug!("Received all data: {} bytes", total_bytes);
                    break;
                }
            },
            Err(e) => {
                error!("Failed to read data: {}", e);
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

    // Отправляем OK только после получения всех данных и терминатора
    stream.write_all(RESP_OK.as_bytes()).await?;
    
    info!(
        "PUT completed: received {} bytes in {} ns, last_byte: 0x{:02X}",
        total_bytes,
        elapsed_ns,
        last_byte
    );

    Ok(())
}
