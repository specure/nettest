


use log::{debug, info};
use crate::client::print::graph_service::{GraphService, MeasurementResult};
use crate::client::print::printer::{print_help, print_test_header, print_test_result};
use crate::client::runnner::run_threads;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};


pub struct CommandLineArgs {
  pub thread_count: usize,
  pub addr: SocketAddr,
  pub use_tls: bool,
  pub use_websocket: bool,
}

#[derive(Clone)]
pub struct Measurement {
    pub measurements: Vec<(u64, u64)>,
    pub failed: bool,
    pub thread_id: usize,
    pub upload_measurements: Vec<(u64, u64)>,
}

#[derive(Default)]
pub struct SharedStats {
   pub download_measurements: Vec<Vec<(u64, u64)>>,
   pub upload_measurements: Vec<Vec<(u64, u64)>>,
}

pub async fn async_main(args: Vec<String>) -> anyhow::Result<()> {

    info!("Starting measurement client...");
    print_test_header();

    if  args.contains(&"-h".to_string())
    || args.contains(&"--help".to_string())
{
    print_help();
}


    let use_tls = args.iter().any(|arg| arg == "-tls");
    let use_websocket = args.iter().any(|arg| arg == "-ws");
    let perf_test = args.iter().any(|arg| arg == "-perf");
    let graphs = args.iter().any(|arg| arg == "-g");

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
    debug!("WebSocket mode: {}", use_websocket);


    let command_line_args = CommandLineArgs {
        thread_count,
        addr,
        use_tls,
        use_websocket,
    };

    let stats: Arc<Mutex<SharedStats>> = Arc::new(Mutex::new(SharedStats::default()));

    let state_refs = run_threads(command_line_args, stats);

 
    if state_refs.iter().any(|s| s.failed) {
        println!(
            "{} threads failed",
            state_refs.iter().filter(|s| s.failed).count()
        );
    }

    if graphs {
        GraphService::print_graph(&state_refs);
    }
    Ok(())
}


pub fn calculate_speed_from_measurements(measurements: Vec<Vec<(u64, u64)>>) -> (f64, f64, f64) {
    if measurements.is_empty() {
        return (0.0, 0.0, 0.0);
    }

    // Находим t* - минимальное время последнего измерения среди всех потоков
    let t_star = measurements
        .iter()
        .filter_map(|m| m.last().map(|(time, _)| *time))
        .min()
        .unwrap_or(0);

    if t_star == 0 {
        return (0.0, 0.0, 0.0);
    }

    let mut total_bytes = 0.0;

    // Для каждого потока k
    for thread_measurements in measurements {
        if thread_measurements.is_empty() {
            continue;
        }

        // Находим l_k - индекс первого измерения >= t*
        let mut l_k = None;
        for (j, (time, _)) in thread_measurements.iter().enumerate() {
            if *time >= t_star {
                l_k = Some(j);
                break;
            }
        }

        // Если не нашли измерение >= t*, используем последнее
        let l_k = l_k.unwrap_or(thread_measurements.len() - 1);

        // Интерполяция согласно RMBT спецификации
        let b_k = if l_k == 0 {
            // Если первое измерение уже >= t*, используем его
            thread_measurements[0].1 as f64
        } else if l_k < thread_measurements.len() {
            // Интерполяция между двумя точками
            let (t_lk_minus_1, b_lk_minus_1) = thread_measurements[l_k - 1];
            let (t_lk, b_lk) = thread_measurements[l_k];
            
            if t_lk > t_lk_minus_1 {
                // b_k = b_k^(l_k-1) + (t* - t_k^(l_k-1)) * (b_k^(l_k) - b_k^(l_k-1)) / (t_k^(l_k) - t_k^(l_k-1))
                let ratio = (t_star - t_lk_minus_1) as f64 / (t_lk - t_lk_minus_1) as f64;
                b_lk_minus_1 as f64 + ratio * (b_lk - b_lk_minus_1) as f64
            } else {
                // Если времена равны, используем последнее значение
                b_lk as f64
            }
        } else {
            // Если l_k указывает за пределы массива, используем последнее измерение
            thread_measurements.last().unwrap().1 as f64
        };

        total_bytes += b_k;
    }

    // Вычисляем скорость R = (1/t*) * Σ(b_k)
    let speed_bps = (total_bytes * 8.0) / (t_star as f64 / 1_000_000_000.0);
    let speed_gbps = speed_bps / 1_000_000_000.0;
    let speed_mbps = speed_bps / 1_000_000.0;

    (speed_bps, speed_gbps, speed_mbps)
}

pub fn calculate_download_speed_from_stats(stats: &Vec<Vec<(u64, u64)>>) -> (f64, f64, f64) {
    let speed = calculate_speed_from_measurements(stats.clone());
    print_test_result("Download Test", "Completed", Some(speed));
    speed
}

pub fn calculate_upload_speed_from_stats(stats: &Vec<Vec<(u64, u64)>>) -> (f64, f64, f64) {
    let speed = calculate_speed_from_measurements(stats.clone());
    print_test_result("Upload Test", "Completed", Some(speed));
    speed
}

pub fn calculate_download_speed(states: &Vec<Measurement>) -> (f64, f64, f64) {
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

pub fn calculate_perf_speed(states: &Vec<Measurement>) -> (f64, f64, f64) {
    let mut thread_measurements: Vec<Vec<(u64, u64)>> = Vec::new();
    for state in states {
        if state.failed {
            continue;
        }
        thread_measurements.push(
            state
                .upload_measurements
                .clone()
                .into_iter()
                .map(|m| (m.0, m.1))
                .collect(),
        );
    }

    calculate_speed_from_measurements(thread_measurements)
}
