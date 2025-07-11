use std::{sync::{Arc, Barrier}, thread};

use log::debug;

use crate::client::{print::printer::print_result, client::CommandLineArgs, client::Measurement, state::TestState};

pub fn run_threads(command_line_args: CommandLineArgs) -> Vec<Measurement> {
    let barrier = Arc::new(Barrier::new(command_line_args.thread_count));
    let mut thread_handles = vec![];

    for i in 0..command_line_args.thread_count {
        let barrier = Arc::clone(&barrier);
        thread_handles.push(thread::spawn(move || {
            let mut state = TestState::new(command_line_args.addr, command_line_args.use_tls, command_line_args.use_websocket, i, None, None).unwrap();
            let greeting = state.process_greeting();
            match greeting {
                Ok(_) => {
                }
                Err(e) => {
                    println!("Greeting error: {:?} token: {}", e, i);
                }
            }
            barrier.wait();
            state.run_get_chunks().unwrap();
            if i == 0 {
                print_result("Get Chunks", "Completed", Some(state.measurement_state().chunk_size as usize));
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
            debug!("Thread {} completed barrier", i);
            state.run_perf_test().unwrap();
            debug!("Upload speed: {}", state.measurement_state().upload_speed.unwrap());
            barrier.wait();
            let result: Measurement = Measurement {
                thread_id: i,
                failed: state.measurement_state().failed,
                measurements: state.measurement_state().measurements.iter().cloned().collect(),
                upload_measurements: state.measurement_state().upload_measurements.iter().cloned().collect(),
            };
            result
        }));
    }

    let states: Vec<Measurement> = thread_handles
        .into_iter()
        .map(|h| h.join().unwrap())
        .collect();

    let state_refs: Vec<Measurement> = states
        .iter()
        //TODO whar to do on failed threads?
        .filter(|s| !s.failed)
        .cloned()
        .collect();

    if state_refs.len() != command_line_args.thread_count {
        println!("Failed threads: {}", command_line_args.thread_count - state_refs.len());
    }

    state_refs
    
}