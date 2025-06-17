use log::info;
use mio::{Events, Poll};
use std::{net::SocketAddr, path::Path};
use std::thread;
use std::collections::HashMap;
use anyhow::Result;
use std::env;
use measurement_client::Stream;
use std::sync::{Arc, Barrier};
use tokio::sync::Mutex;
use fastrand;

mod handlers;
mod state;
use state::{TestState, MeasurementState};
pub mod rustls;
pub mod openssl_sys;
pub mod openssl;
pub mod stream;
pub mod utils;
pub mod globals;

pub use handlers::{
    get_chunks::GetChunksHandler, greeting::GreetingHandler, put::PutHandler,
    put_no_result::PutNoResultHandler,
};
pub use utils::{
    read_until, write_all, write_all_nb, DEFAULT_READ_BUFFER_SIZE, DEFAULT_WRITE_BUFFER_SIZE,
    RMBT_UPGRADE_REQUEST,
};

const MIN_CHUNK_SIZE: u64 = 4096; // 4KB
const MAX_CHUNK_SIZE: u64 = 4194304; // 4MB

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

    let perf_test = args.iter().any(|arg| arg == "-perf");

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

    let default_server = String::from("127.0.0.1");
    let server = args.get(1).unwrap_or(&default_server);


    info!("Server: {}", server);

    let addr = if !use_tls {
        format!("{}:5005", server).parse::<SocketAddr>()?
    } else {
        format!("{}:8080", server).parse::<SocketAddr>()?
    };

    info!("Connected to server at {}", addr);


    if perf_test {
        info!("Running performance test");
        let mut state = TestState::new(addr, use_tls, 0, None, None).unwrap();
            state.process_greeting().unwrap();
            state.run_perf_test().unwrap();
            return Ok(());
    }
    // let addr = "127.0.0.1:5005".parse::<SocketAddr>()?;

    let barrier = Arc::new(Barrier::new(thread_count));
    let mut thread_handles = vec![];



    for i in 0..thread_count {
        let barrier = barrier.clone();
        thread_handles.push(thread::spawn(move || {
            let mut state = TestState::new(addr, use_tls, i, None, None).unwrap();
            state.process_greeting().unwrap();
            barrier.wait(); 
            state.run_get_chunks().unwrap();
            barrier.wait();
            if i == 0 {
                state.run_ping().unwrap();
            }
            barrier.wait();
            state.run_get_time().unwrap();
            barrier.wait(); 
            state.run_put_no_result().unwrap();
            barrier.wait(); 
            state.run_put().unwrap();
            barrier.wait(); 
            let result = MeasurementState {
                upload_results_for_graph: state.measurement_state().upload_results_for_graph.clone(),
                upload_bytes: state.measurement_state().upload_bytes,
                upload_time: state.measurement_state().upload_time,
                upload_speed: state.measurement_state().upload_speed,
                download_time: state.measurement_state().download_time,
                download_bytes: state.measurement_state().download_bytes,
                download_speed: state.measurement_state().download_speed,
                chunk_size: state.measurement_state().chunk_size,
                ping_median: state.measurement_state().ping_median,
                read_buffer_temp: state.measurement_state().read_buffer_temp.clone(),
                measurements: state.measurement_state().measurements.clone(),
                phase: state.measurement_state().phase.clone(),
                buffer: state.measurement_state().buffer.clone(),
            };
            result
        }));
    }

    let states: Vec<MeasurementState> = thread_handles.into_iter().map(|h| h.join().unwrap()).collect();
    let state_refs: Vec<&MeasurementState> = states.iter().collect();
    calculate_download_speed(&state_refs);
    calculate_upload_speed(&state_refs);

    info!("Test completed");

    Ok(())
}

fn calculate_speed_from_measurements(measurements: Vec<Vec<(u64, u64)>>) -> (f64, f64, f64) {
    if measurements.is_empty() {
        return (0.0, 0.0, 0.0);
    }

    // Находим t* - минимальное время последнего измерения
    let t_star = measurements
        .iter()
        .filter_map(|m| m.last().map(|(time, _)| *time))
        .min()
        .unwrap_or(0);

    let mut total_bytes = 0.0;

    // Для каждого потока
    for thread_measurements in measurements {
        if thread_measurements.is_empty() {
            continue;
        }

        // Находим последнее измерение до t*
        let mut last_valid_idx = 0;
        for (i, (time, _)) in thread_measurements.iter().enumerate() {
            if *time <= t_star {
                last_valid_idx = i;
            } else {
                break;
            }
        }

        // Если есть следующее измерение после t*, интерполируем
        if last_valid_idx + 1 < thread_measurements.len() {
            let (t1, b1) = thread_measurements[last_valid_idx];
            let (t2, b2) = thread_measurements[last_valid_idx + 1];
            
            // Интерполяция: b = b1 + (t* - t1) * (b2 - b1) / (t2 - t1)
            let ratio = (t_star - t1) as f64 / (t2 - t1) as f64;
            let interpolated_bytes = b1 as f64 + ratio * (b2 - b1) as f64;
            total_bytes += interpolated_bytes;
        } else {
            // Если нет следующего измерения, используем последнее известное
            total_bytes += thread_measurements[last_valid_idx].1 as f64;
        }
    }

    // Вычисляем скорость R = (1/t*) * Σ(b_k)
    let speed_bps = (total_bytes * 8.0) / (t_star as f64 / 1_000_000_000.0);
    let speed_gbps = speed_bps / 1_000_000_000.0;
    let speed_mbps = speed_bps / 1_000_000.0;

    (speed_bps, speed_gbps, speed_mbps)
}

fn calculate_download_speed(states: &[&MeasurementState]) {
    let mut thread_measurements: Vec<Vec<(u64, u64)>> = Vec::new();
    
    for state in states {
        thread_measurements.push(state.measurements.clone().into_iter().map(|m| (m.0, m.1)).collect());
    }

    let (speed_bps, speed_gbps, speed_mbps) = calculate_speed_from_measurements(thread_measurements);
    println!("Download speed: {:.2} bytes/s, {:.2} Gbit/s, {:.2} Mbit/s", 
        speed_bps / 8.0, speed_gbps, speed_mbps);
}

fn calculate_upload_speed(states: &[&MeasurementState]) {
    let mut results: HashMap<u32, Vec<(u64, u64)>> = HashMap::new();
    
    for (thread_id, state) in states.iter().enumerate() {
        for line in state.read_buffer_temp.split(|&b| b == b'\n') {
            if line.is_empty() { continue; }
            let s = String::from_utf8_lossy(line);
            let parts: Vec<&str> = s.split_whitespace().collect();
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

    let thread_measurements: Vec<Vec<(u64, u64)>> = results.into_values().collect();
    let (speed_bps, speed_gbps, speed_mbps) = calculate_speed_from_measurements(thread_measurements);
    println!("Uplink speed (interpolated, all threads): {:.2} Gbit/s ({:.2} bps, {:.2} MBit/s)",
        speed_gbps, speed_bps, speed_mbps);
}



