use log::debug;
use textplots::Plot;
use textplots::{Chart, Shape};

use crate::client::client::Measurement;

#[derive(Debug, Clone)]
pub struct MeasurementResult {
    pub thread_id: usize,
    pub measurements: Vec<(u64, u64)>, // (time_ns, bytes)
}

pub struct GraphService;

impl GraphService {
    pub fn print_graph(state_refs: &Vec<Measurement>) {
        let download_results: Vec<MeasurementResult> = state_refs
            .iter()
            .enumerate()
            .filter_map(|(thread_id, state)| {
                if !state.measurements.is_empty() {
                    Some(MeasurementResult {
                        thread_id,
                        measurements: state.measurements.iter().cloned().collect(),
                    })
                } else {
                    None
                }
            })
            .collect();

        let upload_results: Vec<MeasurementResult> = state_refs
            .iter()
            .enumerate()
            .filter_map(|(thread_id, state)| {
                if !state.upload_measurements.is_empty() {
                    Some(MeasurementResult {
                        thread_id,
                        measurements: state.upload_measurements.iter().cloned().collect(),
                    })
                } else {
                    None
                }
            })
            .collect();

        // let upload_speed = calculate_upload_speed(&state_refs);

        // Отрисовываем графики используя GraphService
        if !download_results.is_empty()  {
            GraphService::print_download(&download_results);
        }

        if !upload_results.is_empty()  {
            GraphService::print_upload(&upload_results);
        }
    }

     fn print_download(measurement_results: &[MeasurementResult]) {
        println!("\n=== DOWNLOAD SPEED GRAPH ===");
        Self::print_speed_graph(measurement_results, "Download");
    }

     fn print_upload(measurement_results: &[MeasurementResult]) {
        println!("\n=== UPLOAD SPEED GRAPH ===");
        Self::print_speed_graph(measurement_results, "Upload");
    }

    fn print_speed_graph(measurement_results: &[MeasurementResult], test_type: &str) {
        if measurement_results.is_empty() {
            debug!("No data to display");
            return;
        }

        debug!("[DEBUG] Raw measurement_results:");
        for result in measurement_results {
            debug!("  Thread {}:", result.thread_id);
            for (time, bytes) in &result.measurements {
                debug!("    time_ns: {}, bytes: {}", time, bytes);
            }
        }

        let (min_time, max_time) = Self::get_time_range(measurement_results);
        let time_range = max_time - min_time;
        if time_range == 0 {
            debug!("Not enough data to plot the graph");
            return;
        }

        let speed_data = Self::calculate_speeds_per_second(measurement_results, min_time, max_time);
        if speed_data.is_empty() {
            println!("No speed data");
            return;
        }

        debug!("[DEBUG] speed_data (second, Mbit/s, bytes):");
        for (s, mbps, bytes) in &speed_data {
            debug!("  second {:.1}: {:.2} Mbit/s, {} bytes", s, mbps, bytes);
        }
        let plot_data: Vec<(f64, f64)> =
            speed_data.iter().map(|(s, mbps, _)| (*s, *mbps)).collect();
        debug!("[DEBUG] plot_data (second, Mbit/s):");
        for (s, mbps) in &plot_data {
            debug!("  second {:.1}: {:.2} Mbit/s", s, mbps);
        }
        Self::print_speed_textplot(&plot_data, test_type);
    }

    fn get_time_range(measurement_results: &[MeasurementResult]) -> (u64, u64) {
        let mut min_time = u64::MAX;
        let mut min_last_time = u64::MAX;

        for result in measurement_results {
            for (time, _) in &result.measurements {
                min_time = min_time.min(*time);
            }
            if let Some((last_time, _)) = result.measurements.last() {
                min_last_time = min_last_time.min(*last_time);
            }
        }

        (min_time, min_last_time)
    }

    fn calculate_speeds_per_second(
        measurement_results: &[MeasurementResult],
        min_time: u64,
        max_time: u64,
    ) -> Vec<(f64, f64, u64)> {
        let step_ns = 200_000_000u64; // 0.2 сек в наносекундах
        let duration_ns = max_time - min_time;
        let steps = (duration_ns as f64 / step_ns as f64).ceil() as usize;
        let mut speed_data = Vec::new();

        for i in 2..=steps {
            let t = 1.0 + (i - 2) as f64 * 0.2;
            let target_time = min_time + (i as u64 * step_ns);
            let mut total_bytes = 0.0f64;

            for result in measurement_results {
                if result.measurements.is_empty() {
                    continue;
                }
                let mut before = None;
                let mut after = None;

                for (tt, b) in &result.measurements {
                    if *tt <= target_time {
                        before = Some((*tt, *b));
                    }
                    if *tt >= target_time {
                        after = Some((*tt, *b));
                        break;
                    }
                }

                let bytes = match (before, after) {
                    (Some((t0, b0)), Some((t1, b1))) if t1 > t0 => {
                        let dt = t1 - t0;
                        let db = b1 - b0;
                        let dt_target = target_time - t0;
                        b0 as f64 + (dt_target as f64 / dt as f64) * db as f64
                    }
                    (Some((_, b)), None) => b as f64,
                    (None, Some((_, _))) => 0.0 as f64,
                    _ => 0.0,
                };

                debug!("bytes: {}", bytes);
                total_bytes += bytes;
            }

            let speed_mbit = if t > 0.0 {
                total_bytes * 8.0 / t / 1_000_000.0
            } else {
                0.0
            };
            debug!("t: {}, speed_mbit: {}", t, speed_mbit);
            speed_data.push((t, speed_mbit, total_bytes as u64));
        }
        speed_data
    }

    fn print_speed_textplot(speed_data: &[(f64, f64)], test_type: &str) {
        if speed_data.is_empty() {
            return;
        }
        let min_x = 0.0f32;
        let max_x = speed_data.last().unwrap().0 as f32;
        let points: Vec<(f32, f32)> = speed_data
            .iter()
            .map(|(x, y)| (*x as f32, *y as f32))
            .collect();

        println!("{} Test - Speed over Time", test_type);
        Chart::new(192, 48, min_x, max_x)
            .lineplot(&Shape::Lines(&points))
            .display();
        println!("X axis: Seconds, Y axis: Mbit/s");
    }
}
