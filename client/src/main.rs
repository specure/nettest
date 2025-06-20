use anyhow::Result;
use fastrand;
use log::{debug, info};
use measurement_client::Stream;
use mio::{Events, Poll};
use prettytable::format::{FormatBuilder, LinePosition, LineSeparator};
use prettytable::{cell, row, Table};
use std::collections::HashMap;
use std::env;
use std::net::SocketAddr;
use std::sync::{Arc, Barrier};
use std::thread;

mod handlers;
mod state;
use state::{MeasurementState, TestState};
pub mod globals;
pub mod openssl;
pub mod openssl_sys;
pub mod rustls;
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

const MIN_CHUNK_SIZE: u64 = 4096; // 4KB
const MAX_CHUNK_SIZE: u64 = 4194304; // 4MB

const GREEN: &str = "\x1b[32m";
const RESET: &str = "\x1b[0m";

#[tokio::main]
async fn main() -> Result<()> {
    async_main().await
}

async fn async_main() -> anyhow::Result<()> {
    info!("Starting measurement client...");
    print_test_header();

    // Парсим аргументы командной строки
    let args: Vec<String> = env::args().collect();
    let use_tls = args.iter().any(|arg| arg == "-tls");
    let perf_test = args.iter().any(|arg| arg == "-perf");

    let log = args.iter().any(|arg| arg == "-log");
    if log {
        env_logger::init();
    }

    let def_t = if perf_test { 1 } else { 5 };

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
        .unwrap_or(def_t);

    let default_server = String::from("127.0.0.1");
    let server = args.get(1).unwrap_or(&default_server);

    let addr = if !use_tls {
        format!("{}:5005", server).parse::<SocketAddr>()?
    } else {
        format!("{}:8080", server).parse::<SocketAddr>()?
    };

    debug!("Addr : {}", addr);

    if perf_test {
        let mut thread_handles = vec![];

        for i in 0..thread_count {
            thread_handles.push(thread::spawn(move || {
                let mut state = TestState::new(addr, use_tls, i, None, None).unwrap();
                state.process_greeting().unwrap();
                state.run_perf_test().unwrap();
                let speed = state.measurement_state().upload_speed.unwrap() * 8.0
                    / (1024.0 * 1024.0 * 1024.0);
                let mbps = state.measurement_state().upload_speed.unwrap() / (1024.0 * 1024.0);

                if thread_count > 1 {
                print_test_result(
                    format!("Performance Test {:?} Thread", i+1).as_str(),
                    "Completed (Gbit/s)",
                    Some((0.0, speed, mbps)),
                );
            }
                speed
            }));

        }
        let res: Vec<f64> = thread_handles
            .into_iter()
            .map(|h| h.join().unwrap())
            .collect();

        let speed = res.iter().sum::<f64>();
        let mbps = speed / 8.0 * 1024.0;

        print_test_result(
            "Performance Test (Sum)",
            "Completed (Gbit/s)",
            Some((0.0, speed, mbps)),
        );
        return Ok(());
    }

    let barrier = Arc::new(Barrier::new(thread_count));
    let mut thread_handles = vec![];

    for i in 0..thread_count {
        let barrier = barrier.clone();
        thread_handles.push(thread::spawn(move || {
            let mut state = TestState::new(addr, use_tls, i, None, None).unwrap();
            state.process_greeting().unwrap();
            barrier.wait();
            state.run_get_chunks().unwrap();
            if i == 0 {
                print_result(
                    "Pre Download",
                    "Chunk Size (bytes)",
                    Some(state.measurement_state().chunk_size),
                );
            }
            barrier.wait();
            if i == 0 {
                state.run_ping().unwrap();
                let median = state.measurement_state().ping_median.unwrap();
                print_result("Ping Median", "Completed (ns)", Some(median as usize));
            }
            barrier.wait();
            state.run_get_time().unwrap();
            barrier.wait();
            state.run_put_no_result().unwrap();
            if i == 0 {
                print_result(
                    "Pre Upload",
                    "Chunk Size (bytes)",
                    Some(state.measurement_state().chunk_size),
                );
            }
            barrier.wait();
            state.run_put().unwrap();
            barrier.wait();
            let result = MeasurementState {
                upload_results_for_graph: state
                    .measurement_state()
                    .upload_results_for_graph
                    .clone(),
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
                phase_start_time: state.measurement_state().phase_start_time,
                failed: state.measurement_state().failed,
            };
            result
        }));
    }

    let states: Vec<MeasurementState> = thread_handles
        .into_iter()
        .map(|h| h.join().unwrap())
        .collect();
    let state_refs: Vec<&MeasurementState> = states.iter().collect();

    let download_speed = calculate_download_speed(&state_refs);

    print_test_result("Download Test", "Completed", Some(download_speed));

    let upload_speed = calculate_upload_speed(&state_refs);
    print_test_result("Upload Test", "Completed", Some(upload_speed));

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

fn calculate_download_speed(states: &[&MeasurementState]) -> (f64, f64, f64) {
    let mut thread_measurements: Vec<Vec<(u64, u64)>> = Vec::new();
    for state in states {
        if state.failed {
            continue;
        }
        thread_measurements.push(
            state
                .measurements
                .clone()
                .into_iter()
                .map(|m| (m.0, m.1))
                .collect(),
        );
    }

    calculate_speed_from_measurements(thread_measurements)
}

fn calculate_upload_speed(states: &[&MeasurementState]) -> (f64, f64, f64) {
    let mut results: HashMap<u32, Vec<(u64, u64)>> = HashMap::new();

    for (thread_id, state) in states.iter().enumerate() {
        if state.failed {
            continue;
        }
        for line in state.read_buffer_temp.split(|&b| b == b'\n') {
            debug!("Line: {:?}", String::from_utf8_lossy(line));
            if line.is_empty() {
                continue;
            }
            let s = String::from_utf8_lossy(line);
            let parts: Vec<&str> = s.split_whitespace().collect();
            if parts.len() >= 4 && parts[0] == "TIME" && parts[2] == "BYTES" {
                if let (Ok(time_ns), Ok(bytes)) = (parts[1].parse::<u64>(), parts[3].parse::<u64>())
                {
                    results
                        .entry(thread_id as u32)
                        .or_default()
                        .push((time_ns, bytes));
                }
            }
        }
    }

    let thread_measurements: Vec<Vec<(u64, u64)>> = results.into_values().collect();
    calculate_speed_from_measurements(thread_measurements)
}

fn print_test_header() {
    // Print centered green title
    let title = "Nettest Broadband Test";
    let table_width = 74; // 30 + 40 + 4 (padding and borders)
    let padding = (table_width - title.len()) / 2;
    println!(
        "\n{}{}{}{}{}",
        " ".repeat(padding),
        GREEN,
        title,
        RESET,
        " ".repeat(padding)
    );
    println!();

    let mut table = Table::new();
    let format = FormatBuilder::new()
        .column_separator('│')
        .borders('│')
        .separators(
            &[LinePosition::Top, LinePosition::Bottom],
            LineSeparator::new('─', '┼', '┌', '┐'),
        )
        .padding(1, 1)
        .build();
    table.set_format(format);

    table.add_row(row![
        format!("{:<30}", "Test Phase"),
        format!("{:<40}", "Result")
    ]);
    println!("{}", table);
}

fn print_test_result(phase: &str, status: &str, speed: Option<(f64, f64, f64)>) {
    let mut table = Table::new();
    let format = FormatBuilder::new()
        .column_separator('│')
        .borders('│')
        .separators(
            &[LinePosition::Bottom],
            LineSeparator::new('─', '┼', '├', '┤'),
        )
        .padding(1, 1)
        .build();
    table.set_format(format);

    let result = if let Some((_, gbps, mbps)) = speed {
        format!("{} - {:.2} Gbit/s ({:.2} Mbit/s)", status, gbps, mbps)
    } else {
        status.to_string()
    };

    table.add_row(row![format!("{:<30}", phase), format!("{:<40}", result)]);
    println!("{}", table);
}

fn print_result(phase: &str, status: &str, speed: Option<(usize)>) {
    let mut table = Table::new();
    let format = FormatBuilder::new()
        .column_separator('│')
        .borders('│')
        .separators(
            &[LinePosition::Bottom],
            LineSeparator::new('─', '┼', '├', '┤'),
        )
        .padding(1, 1)
        .build();
    table.set_format(format);

    let result = if let Some((mbps)) = speed {
        format!("{} - {:.2} ", status, mbps)
    } else {
        status.to_string()
    };

    table.add_row(row![format!("{:<30}", phase), format!("{:<40}", result)]);
    println!("{}", table);
}
