pub mod config;
pub mod handlers;
pub mod logger;
pub mod stream;
pub mod utils;

use log::{debug, info};
use rustls::pki_types::{IpAddr, ServerName};

use std::{error::Error, net::SocketAddr, sync::Arc};

use tokio::{net::TcpStream, sync::Barrier};

use crate::stream::Stream;
use std::env;

struct MeasurementResult {
    ping: u64,
    upload_bytes: u64,
    download_bytes: u64,
    upload_time: u64,
    download_time: u64,
}

/// Запускает один поток измерения
async fn run_single_measurement(use_tls: bool, use_websocket: bool, thread_id: usize, barrier: Arc<Barrier>) -> Result<MeasurementResult, Box<dyn Error + Send + Sync>> {
    let default_server = String::from("127.0.0.1");
    
    let (mut stream, addr) = if use_tls {
        let addr = format!("{}:8080", default_server).parse::<SocketAddr>()?;
        let stream = TcpStream::connect(addr).await?;
        let tls_connector = utils::tls::load_identity()?;
        let domain = ServerName::try_from("localhost")?;
        let stream = tls_connector.connect(domain, stream).await?;
        (Stream::Tls(stream), addr)
    } else {
        let addr = format!("{}:5005", default_server).parse::<SocketAddr>()?;
        let stream = TcpStream::connect(addr).await?;
        (Stream::Plain(stream), addr)
    };

    if use_websocket {
        let key = String::from_utf8_lossy(b"dGhlIHNhbXBsZSBub25jZQ==");
        debug!("WebSocket key: {}", key);
        stream = stream.upgrade_to_websocket().await?;
    }

    let mut result = MeasurementResult {
        ping: 0,
        upload_bytes: 0,
        download_bytes: 0,
        upload_time: 0,
        download_time: 0,
    };

    handlers::handle_greeting(&mut stream, use_websocket).await?;
    barrier.wait().await;
    handlers::handle_get_time(&mut stream, &mut result).await?;
    barrier.wait().await;
    handlers::handle_put_no_result(&mut stream, &mut result).await?;

    // Здесь можно добавить логику для получения результатов измерений
    // Пока возвращаем заглушки

    println!("Upload: {} bytes in {} ns", result.upload_bytes, result.upload_time);

    Ok((result))
}

/// Запускает многопоточный тест
async fn run_multithreaded_test(use_tls: bool, use_websocket: bool, thread_count: usize) -> Result<(), Box<dyn Error + Send + Sync>> {
    println!("\n=== Многопоточный тест ({} потоков) ===", thread_count);
    
    let mut handles = vec![];

    let barrier = Arc::new(tokio::sync::Barrier::new(thread_count));
    
    // Запускаем все потоки одновременно
    for i in 0..thread_count {
        println!("Starting thread {}", i);
        let handle = tokio::spawn(run_single_measurement(use_tls, use_websocket, i, barrier.clone()));
        handles.push(handle);
    }
    
    // Ждем завершения всех потоков и собираем результаты
    let mut results = vec![];
    for handle in handles {
        let result = handle.await??; // Двойной unwrap для JoinHandle и Result
        results.push(result);
    }

    let total_upload_speed = results.iter().map(|r| r.upload_bytes as f64 * 8.0 / r.upload_time as f64 * 1000000000.0).sum::<f64>();
    let total_download_speed = results.iter().map(|r| r.download_bytes as f64 * 8.0 / r.download_time as f64 * 1000000000.0).sum::<f64>();
    let avg_ping = results.iter().map(|r| r.ping).sum::<u64>() / thread_count as u64;
    
    
    println!("\nИтоговые результаты ({} потоков):", thread_count);
    println!("  Upload: {:.2} Gbit/s", total_upload_speed / 1_000_000_000.0);
    println!("  Download: {:.2} Gbit/s", total_download_speed / 1_000_000_000.0);
    println!("  Ping: {} ns", avg_ping);
    
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let args: Vec<String> = env::args().collect();
    let use_tls = args.iter().any(|arg| arg == "-tls");
    let use_websocket = args.iter().any(|arg| arg == "-ws");
    let perf_test = args.iter().any(|arg| arg == "-perf");

    let log = args.iter().any(|arg| arg == "-log");
    if log {
        env_logger::init();
    }

    // Получаем количество потоков из аргументов
    let thread_count = args
        .iter()
        .enumerate()
        .find_map(|(i, arg)| {
            if arg == "-t" {
                args.get(i + 1).and_then(|next| next.parse::<usize>().ok())
            } else if arg.starts_with("-t") {
                arg[2..].parse::<usize>().ok()
            } else {
                None
            }
        })
        .unwrap_or(3); // По умолчанию 3 потока

    let default_server = String::from("127.0.0.1");

    // Запускаем многопоточный тест
    run_multithreaded_test(use_tls, use_websocket, thread_count).await?;

    println!("Hello, world!");
    Ok(())
}
 