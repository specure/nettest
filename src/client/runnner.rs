use std::{
    sync::{Arc, Barrier},
    thread,
};

use crate::config::parser::parse_listen_address;
use std::sync::Mutex;

use log::{debug, info};

use crate::client::{
    calculator::{
        calculate_download_speed_from_stats_silent, calculate_upload_speed_from_stats_silent,
    },
    client::{ClientConfig, Measurement, SharedStats},
    control_server::MeasurementSaver,
    print::printer::{print_float_result, print_test_result},
    state::TestState,
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

    debug!(
        "config.port: {}, config.tls_port: {}",
        config.port, config.tls_port
    );

    let addr = if !config.use_tls {
        parse_listen_address(&format!("{}:{}", ip, config.port)).unwrap()
    } else {
        parse_listen_address(&format!("{}:{}", ip, config.tls_port)).unwrap()
    };

    for i in 0..config.thread_count {
        let barrier = Arc::clone(&barrier);
        let stats = Arc::clone(&stats);
        let ping_median_clone = Arc::clone(&ping_median);
        let download_speed_clone = Arc::clone(&download_speed);
        let upload_speed_clone = Arc::clone(&upload_speed);
        thread_handles.push(thread::spawn(move || {
            let mut state =
                match TestState::new(addr, config.use_tls, config.use_websocket, i, None, None) {
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
                    println!("Thread {i} could not connect to the server. {:?}", e);
                    return Err(anyhow::anyhow!("Greeting failed with error: {:?}", e));
                }
            }
            barrier.wait();
            state.run_get_chunks().unwrap();

            barrier.wait();

            if i == 0 {
                state.run_ping().unwrap();
                let ping_ms = state.measurement_state().ping_median
                    .map(|m| m as f64 / 1_000_000.0);
                if let Some(median) = state.measurement_state().ping_median {
                    *ping_median_clone.lock().unwrap() = Some(median);
                }
                if config.raw_output {
                    if let Some(p) = ping_ms { print!("{:.2}", p); }
                } else {
                    print_float_result("Ping Median", "ms", ping_ms, false);
                }

                if !config.legacy {
                    if let Err(e) = state.run_voip_test() {
                        log::warn!("VoIP test failed: {}", e);
                    } else {
                        let ms = state.measurement_state();
                        let jitter = match (&ms.voip_result_in, &ms.voip_result_out) {
                            (Some(i), Some(o)) => Some(i.mean_jitter.max(o.mean_jitter) as f64 / 1_000_000.0),
                            (Some(i), None)    => Some(i.mean_jitter as f64 / 1_000_000.0),
                            (None,    Some(o)) => Some(o.mean_jitter as f64 / 1_000_000.0),
                            (None,    None)    => None,
                        };
                        print_float_result("Jitter", "ms", jitter, false);
                    }

                    let pre_udp_failed = state.measurement_state().failed;
                    if let Err(e) = state.run_udp_test() {
                        log::warn!("UDP packet loss test failed: {}", e);
                    }
                    // UDP failure is non-critical — don't mark thread as failed
                    if !pre_udp_failed {
                        state.reset_failed();
                    }
                    let ms = state.measurement_state();
                    let loss = match (&ms.udp_result_out, &ms.udp_result_in) {
                        (Some(o), Some(i)) => Some(o.packet_loss_rate.max(i.packet_loss_rate) as f64),
                        (Some(o), None)    => Some(o.packet_loss_rate as f64),
                        (None,    Some(i)) => Some(i.packet_loss_rate as f64),
                        (None,    None)    => None,
                    };
                    if loss.is_some() {
                        print_float_result("Packet Loss", "%", loss, false);
                    }
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
                let speed =
                    calculate_download_speed_from_stats_silent(&stats_guard.download_measurements);

                // Save download speed for later use
                *download_speed_clone.lock().unwrap() = Some(speed.2); // speed.1 is Gbps

                if config.raw_output {
                    print!("/{:.2}", speed.1); // speed.1 is Gbps
                } else {
                    print_test_result("Download Test", "Completed", Some(speed), false);
                }
            }

            barrier.wait();

            if config.legacy {
                state.run_put().unwrap();
            } else {
                state.run_perf_test().unwrap();
            }
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
                let speed =
                    calculate_upload_speed_from_stats_silent(&stats_guard.upload_measurements);

                // Save upload speed for later use
                *upload_speed_clone.lock().unwrap() = Some(speed.2); // speed.1 is Gbps

                if config.raw_output {
                    println!("/{:.2}", speed.1); // speed.1 is Gbps, println! for line break
                } else {
                    print_test_result("Upload Test", "Completed", Some(speed), true);
                }
            }

            barrier.wait();

            if config.save_results && config.signed_result {
                state.run_signed_result().unwrap();
                barrier.wait();
            }

            let ms = state.measurement_state();
            let result: Measurement = Measurement {
                thread_id: i,
                failed: ms.failed,
                phase: ms.phase.clone(),
                measurements: ms.download_measurements.iter().cloned().collect(),
                upload_measurements: ms.upload_measurements.iter().cloned().collect(),
                envelope: ms.envelope.clone(),
                ping_median_ns: ms.ping_median,
                voip_result_in: ms.voip_result_in.clone(),
                voip_result_out: ms.voip_result_out.clone(),
                udp_result_out: ms.udp_result_out.clone(),
                udp_result_in: ms.udp_result_in.clone(),
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

    for s in states.iter() {
        if s.failed {
            info!("Failed thread {} on phase {:?}", s.thread_id, s.phase);
        }
    }

    let state_refs: Vec<Measurement> = states
        .iter()
        //TODO whar to do on failed threads?
        .filter(|s| !s.failed)
        .cloned()
        .collect();

    if state_refs.len() != config.thread_count {
        println!("Failed threads: {} out of {}", config.thread_count - state_refs.len(), config.thread_count);
    }

    let envelopes: Vec<Option<String>> = state_refs.iter().map(|s| s.envelope.clone()).collect();

    // Save results if -save option is enabled
    if config.save_results {
        let mut measurement_saver = MeasurementSaver::new(&config_clone);

        // Get all saved values
        let ping_median_value = *ping_median.lock().unwrap();
        let download_speed_value = *download_speed.lock().unwrap();
        let upload_speed_value = *upload_speed.lock().unwrap();

        if let Err(e) = measurement_saver
            .save_measurement_with_speeds(
                ping_median_value,
                download_speed_value,
                upload_speed_value,
                envelopes,
            )
            .await
        {
            eprintln!("Failed to save measurement: {}", e);
        }
    }

    Ok(state_refs)
}
