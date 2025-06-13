use log::info;
use mio::{Events, Poll};
use std::{net::SocketAddr, path::Path};
use std::thread;
use std::collections::HashMap;
use anyhow::Result;
use std::env;
use measurement_client::Stream;

mod handlers;
mod state;
use state::{TestState, MeasurementState};
pub mod rustls;
pub mod openssl_sys;
pub mod openssl;
pub mod stream;
pub mod utils;

pub use handlers::{
    get_chunks::GetChunksHandler, greeting::GreetingHandler, put::PutHandler,
    put_no_result::PutNoResultHandler,
};
pub use utils::{
    read_until, write_all, write_all_nb, DEFAULT_READ_BUFFER_SIZE, DEFAULT_WRITE_BUFFER_SIZE,
    RMBT_UPGRADE_REQUEST,
};

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    async_main().await
}

async fn async_main() -> anyhow::Result<()> {
    info!("Starting measurement client...");
    
    // Парсим аргументы командной строки
    let args: Vec<String> = env::args().collect();
    let use_tls = args.iter().any(|arg| arg == "-tls");
    
    // Получаем количество потоков из аргументов
    let thread_count = args.iter()
        .enumerate()
        .find_map(|(i, arg)| {
            if arg == "-t" {
                // Формат: -t 1
                args.get(i + 1).and_then(|next| next.parse::<usize>().ok())
            } else if arg.starts_with("-t") {
                // Формат: -t1
                arg[2..].parse::<usize>().ok()
            } else {
                None
            }
        })
        .unwrap_or(3); // По умолчанию 3 потока
    
    info!("TLS enabled: {}", use_tls);
    info!("Thread count: {}", thread_count);
    info!("Connecting to server...");

    let addr = "10.35.2.151:8082".parse::<SocketAddr>()?;

    info!("Connected to server at {}", addr);

    let mut handles = Vec::new();
    let mut poll = Poll::new()?;
    // let events = Events::with_capacity(2048);
    for k in 0..thread_count {
        let addr = addr.clone();
        info!("Starting thread {}", k);
        handles.push(thread::spawn(move || {
            let mut state = TestState::new(addr, use_tls, k, None, None).unwrap();
            state.run_measurement().unwrap()
        }));
    }
    let states: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();

    let mut common_buffer = Vec::new();

    let state_refs: Vec<_> = states.iter().collect();
    for state in &states {
        common_buffer.extend_from_slice(&state.read_buffer_temp);
    }

    calculate_upload_speed(&state_refs);

    info!("Test completed");

    Ok(())
}

fn calculate_upload_speed(states: &[&MeasurementState]) {
    let mut results: HashMap<u32, Vec<(u64, u64)>> = HashMap::new();

    info!("Results: {:?}", results);
    
    // Process data from each state's read_buffer_temp
    for (thread_id, state) in states.iter().enumerate() {
        for line in state.read_buffer_temp.split(|&b| b == b'\n') {
            if line.is_empty() { continue; }
            let s = String::from_utf8_lossy(line);
            let parts: Vec<&str> = s.split_whitespace().collect();
            // Format: "TIME <time_ns> BYTES <bytes>"
            if parts.len() >= 4 && parts[0] == "TIME" && parts[2] == "BYTES" {
                if let (Ok(time_ns), Ok(bytes)) = (
                    parts[1].parse::<u64>(),
                    parts[3].parse::<u64>(),
                ) {
                    results.entry(thread_id as u32).or_default().push((time_ns, bytes));
                }
            }
        }
    }

    if results.is_empty() {
        println!("Недостаточно данных для расчета 111 скорости");
        return;
    }

    // t* — минимальное из последних t по всем потокам
    let t_star = results
        .values()
        .filter_map(|v| v.last().map(|(t, _)| *t))
        .min()
        .unwrap();

    let mut total_bytes = 0.0;
    for (_thread_id, v) in &results {
        // Найти два ближайших измерения: t_{l-1} < t* <= t_l
        let mut b_k_star = None;
        for i in 1..v.len() {
            if v[i].0 >= t_star {
                let (t_lm1, b_lm1) = v[i - 1];
                let (t_l, b_l) = v[i];
                let b_star = b_lm1 as f64
                    + (t_star as f64 - t_lm1 as f64) / (t_l as f64 - t_lm1 as f64)
                        * (b_l as f64 - b_lm1 as f64);
                b_k_star = Some(b_star);
                break;
            }
        }
        // Если не нашли интервал — берём последнее значение до t*
        if b_k_star.is_none() {
            if let Some(&(t, b)) = v.iter().rev().find(|&&(t, _)| t <= t_star) {
                b_k_star = Some(b as f64);
            }
        }
        if let Some(b_star) = b_k_star {
            total_bytes += b_star;
        }
    }

    let speed_bps = total_bytes * 8.0 / (t_star as f64 / 1_000_000_000.0);
    let speed_gbps = speed_bps / 1_000_000_000.0;
    let mbps = speed_bps / 1_000_000.0;
    println!(
        "Uplink speed (interpolated, all threads): {:.2} Gbit/s ({:.2} bps, {:.2} MBit/s)",
        speed_gbps, speed_bps, mbps
    );
}
