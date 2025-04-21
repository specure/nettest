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
    if parts.len() > 2 {
        stream.write_all(RESP_ERR.as_bytes()).await?;
        return Err(format!("Invalid number of arguments for PUT: {:?}", parts).into());
    }

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
    stream.flush().await?;

    // Начинаем измерение времени
    let start_time = Instant::now();
    let mut total_bytes = 0;
    let mut last_report_time = 0u64;
    let mut buffer = vec![0u8; chunk_size];
    let mut found_terminator = false;
    let mut last_chunk_time = 0u64;

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

                // Отправляем промежуточный результат только если:
                // 1. Прошло достаточно времени с последнего отчета (r = 0.001 секунды)
                // 2. Получен новый чанк
                let elapsed_ns = start_time.elapsed().as_nanos() as u64;
                if elapsed_ns - last_report_time > 1_000_000 && elapsed_ns - last_chunk_time > 1_000_000 {
                    let time_response = format!("{} {} BYTES {}\n", RESP_TIME, elapsed_ns, total_bytes);
                    stream.write_all(time_response.as_bytes()).await?;
                    stream.flush().await?;
                    last_report_time = elapsed_ns;
                    debug!("Sent intermediate result: TIME {} BYTES {}", elapsed_ns, total_bytes);
                }
                last_chunk_time = elapsed_ns;
            },
            Err(e) => {
                error!("Failed to read data: {}", e);
                stream.write_all(RESP_ERR.as_bytes()).await?;
                return Err(e.into());
            }
        }
    }

    // Отправляем финальный результат
    let elapsed_ns = start_time.elapsed().as_nanos() as u64;
    let time_response = format!("{} {}\n", RESP_TIME, elapsed_ns);
    stream.write_all(time_response.as_bytes()).await?;
    stream.flush().await?;

    info!(
        "PUT completed: received {} bytes in {} ns",
        total_bytes,
        elapsed_ns
    );

    Ok(())
}
