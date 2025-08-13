use std::{
    net::SocketAddr,
    sync::{Arc, Barrier},
    thread,
};

use std::sync::Mutex;

use log::debug;

use crate::client::{
    calculator::{ calculate_download_speed_from_stats_silent, calculate_upload_speed_from_stats_silent}, client::{ClientConfig, Measurement, SharedStats}, print::printer::{print_float_result, print_test_result}, state::TestState, control_server::MeasurementSaver
};

pub async fn run_threads(
    config: ClientConfig,
    stats: Arc<Mutex<SharedStats>>,
) -> Result<Vec<Measurement>, anyhow::Error> {
    let config_clone = config.clone();
    let barrier = Arc::new(Barrier::new(config.thread_count));
    let mut thread_handles = vec![];
    let ping_median = Arc::new(Mutex::new(None::<u64>));
    let download_speed = Arc::new(Mutex::new(None::<f64>));
    let upload_speed = Arc::new(Mutex::new(None::<f64>));

    // Get server address (IP or hostname)
    let server_addr = config.server.unwrap();
    
    // Resolve IP if it's a hostname
    let ip = if crate::client::control_server::servers::is_ip_address(&server_addr) {
        server_addr.clone()
    } else {
        match crate::client::control_server::servers::resolve_ip_from_web_address(&server_addr) {
            Ok(ip) => ip,
            Err(_) => server_addr.clone(), // Fallback to original if resolution fails
        }
    };
    
    debug!("Resolved IP: {}", ip);
    
    let addr = if !config.use_tls {
        format!("{}:{}", ip, config.port).parse::<SocketAddr>()?
    } else {
        format!("{}:{}", ip, config.tls_port).parse::<SocketAddr>()?
    };


    for i in 0..config.thread_count {
        let barrier = Arc::clone(&barrier);
        let stats = Arc::clone(&stats);
        let ping_median_clone = Arc::clone(&ping_median);
        let download_speed_clone = Arc::clone(&download_speed);
        let upload_speed_clone = Arc::clone(&upload_speed);
        thread_handles.push(thread::spawn(move || {

            let mut state = match TestState::new(addr, config.use_tls, config.use_websocket, i, None, None) {
                Ok(state) => state,
                Err(e) => {
                    debug!("TestState error: {:?} token: {}", e, i);
                    return Err(e);
                }
            };

            let greeting = state.process_greeting();
            match greeting {
                Ok(_) => {}
                Err(e) => {
                    debug!("Greeting error: {:?} token: {}", e, i);
                }
            }
            barrier.wait();
            state.run_get_chunks().unwrap();
            // if i == 0 {
            //     print_result(
            //         "Get Chunks",
            //         "Completed",
            //         Some(state.measurement_state().chunk_size as usize),
            //     );
            // }

            barrier.wait();

            if i == 0 {
                state.run_ping().unwrap();
                let median = state.measurement_state().ping_median.unwrap();
                let ping_ms = median as f64 / 1000000.0;
                
                // Сохраняем ping_median для последующего использования
                *ping_median_clone.lock().unwrap() = Some(median);
                
                if config.raw_output {
                    print!("{:.2}", ping_ms);
                } else {
                    print_float_result("Ping Median", "ms", Some(ping_ms));
                }
            }
            barrier.wait();

            state.run_get_time().unwrap();
            {
                let mut stats = stats.lock().unwrap();
                stats.download_measurements.push(
                    state
                        .measurement_state()
                        .download_measurements
                        .iter()
                        .cloned()
                        .collect(),
                );
            } 

            barrier.wait();

            if i == 0 {
                let stats_guard = stats.lock().unwrap();
                let speed = calculate_download_speed_from_stats_silent(&stats_guard.download_measurements);
                
                // Сохраняем download скорость для последующего использования
                *download_speed_clone.lock().unwrap() = Some(speed.1); // speed.1 - это Gbps
                
                if config.raw_output {
                    print!("/{:.2}", speed.1); // speed.1 - это Gbps
                } else {
                    print_test_result("Download Test", "Completed", Some(speed));
                }
            }

            barrier.wait();

            state.run_perf_test().unwrap();
            {
                let mut stats = stats.lock().unwrap();
                stats.upload_measurements.push(
                    state
                        .measurement_state()
                        .upload_measurements
                        .iter()
                        .cloned()
                        .collect(),
                );
            }

            barrier.wait();

            if i == 0 {
                let stats_guard = stats.lock().unwrap();
                let speed = calculate_upload_speed_from_stats_silent(&stats_guard.upload_measurements);
                
                // Сохраняем upload скорость для последующего использования
                *upload_speed_clone.lock().unwrap() = Some(speed.1); // speed.1 - это Gbps
                
                if config.raw_output {
                    println!("/{:.2}", speed.1); // speed.1 - это Gbps, println! для перевода строки
                } else {
                    print_test_result("Upload Test", "Completed", Some(speed));
                }
            }

            let result: Measurement = Measurement {
                thread_id: i,
                failed: state.measurement_state().failed,
                measurements: state
                    .measurement_state()
                    .download_measurements
                    .iter()
                    .cloned()
                    .collect(),
                upload_measurements: state
                    .measurement_state()
                    .upload_measurements
                    .iter()
                    .cloned()
                    .collect(),
            };
            Ok(result)
        }));
    }

    let states: Vec<Measurement> = thread_handles
        .into_iter()
        .map(|h| h.join().unwrap())
        .filter(|s| s.is_ok())
        .map(|s| s.unwrap())
        .collect();

    let state_refs: Vec<Measurement> = states
        .iter()
        //TODO whar to do on failed threads?
        .filter(|s| !s.failed)
        .cloned()
        .collect();

    if state_refs.len() != config.thread_count {
        println!("Failed threads: {}", config.thread_count - state_refs.len());
    }

    // Сохраняем результаты если включена опция -save
    if config.save_results {
        let mut measurement_saver = MeasurementSaver::new(
            config.control_server.clone(),
            &config_clone,
        );
        
        // Получаем все сохраненные значения
        let ping_median_value = *ping_median.lock().unwrap();
        let download_speed_value = *download_speed.lock().unwrap();
        let upload_speed_value = *upload_speed.lock().unwrap();
        
        if let Err(e) = measurement_saver.save_measurement_with_speeds(
            ping_median_value, 
            download_speed_value, 
            upload_speed_value
        ).await {
            eprintln!("Failed to save measurement: {}", e);
        }
    }

    Ok(state_refs)
}
