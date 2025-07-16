use std::{
    net::SocketAddr,
    sync::{Arc, Barrier},
    thread,
};

use std::sync::Mutex;

use log::debug;

use crate::client::{
    calculator::{calculate_download_speed_from_stats, calculate_upload_speed_from_stats},
    client::{ClientConfig, Measurement, SharedStats},
    print::printer::print_result,
    state::TestState,
};

pub fn run_threads(
    config: ClientConfig,
    stats: Arc<Mutex<SharedStats>>,
) -> Result<Vec<Measurement>, anyhow::Error> {
    let barrier = Arc::new(Barrier::new(config.thread_count));
    let mut thread_handles = vec![];

    let addr = if !config.use_tls {
        format!("{}:{}", config.server, config.port).parse::<SocketAddr>()?
    } else {
        format!("{}:{}", config.server, config.tls_port).parse::<SocketAddr>()?
    };


    for i in 0..config.thread_count {
        let barrier = Arc::clone(&barrier);
        let stats = Arc::clone(&stats);
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
            if i == 0 {
                print_result(
                    "Get Chunks",
                    "Completed",
                    Some(state.measurement_state().chunk_size as usize),
                );
            }

            // barrier.wait();

            // if i == 0 {
            //     state.run_ping().unwrap();
            //     let median = state.measurement_state().ping_median.unwrap();
            //     print_result("Ping Median", "Completed (ns)", Some(median as usize));
            // }
            // barrier.wait();

            // state.run_get_time().unwrap();
            // {
            //     let mut stats = stats.lock().unwrap();
            //     stats.download_measurements.push(
            //         state
            //             .measurement_state()
            //             .measurements
            //             .iter()
            //             .cloned()
            //             .collect(),
            //     );
            // } 

            // barrier.wait();

            // if i == 0 {
            //     let stats_guard = stats.lock().unwrap();
            //     calculate_download_speed_from_stats(&stats_guard.download_measurements);
            // }



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
                calculate_upload_speed_from_stats(&stats_guard.upload_measurements);
            }

            let result: Measurement = Measurement {
                thread_id: i,
                failed: state.measurement_state().failed,
                //TODO: handle additional clone
                measurements: state
                    .measurement_state()
                    .measurements
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

    Ok(state_refs)
}
