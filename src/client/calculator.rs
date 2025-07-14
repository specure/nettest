use crate::client::{client::Measurement, print::printer::print_test_result};


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