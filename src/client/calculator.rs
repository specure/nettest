use crate::client::{client::Measurement, print::printer::print_test_result};


pub fn calculate_speed_from_measurements(measurements: Vec<Vec<(u64, u64)>>) -> (f64, f64, f64) {
    if measurements.is_empty() {
        return (0.0, 0.0, 0.0);
    }

    let skip_time_ns = 1_000_000_000u64; // 1 секунда в наносекундах
    
    // Находим минимальное время начала измерения
    let min_start_time = measurements
        .iter()
        .filter_map(|m| m.first().map(|(time, _)| *time))
        .min()
        .unwrap_or(0);

    // Находим t* - минимальное время последнего измерения среди всех потоков
    let t_star_original = measurements
        .iter()
        .filter_map(|m| m.last().map(|(time, _)| *time))
        .min()
        .unwrap_or(0);

    if t_star_original == 0 {
        return (0.0, 0.0, 0.0);
    }

    // t* с учетом пропуска первых 2 секунд
    let t_star = t_star_original - skip_time_ns;
    
    // Если после пропуска 2 секунд времени недостаточно, возвращаем 0
    if t_star <= 0 {
        return (0.0, 0.0, 0.0);
    }

    let mut total_bytes = 0.0;

    // Для каждого потока k
    for thread_measurements in measurements {
        if thread_measurements.is_empty() {
            continue;
        }

        // Интерполируем данные в начале (после пропуска 2 секунд)
        let bytes_at_start = interpolate_bytes_at_time(&thread_measurements, min_start_time + skip_time_ns);
        
        // Находим l_k - индекс первого измерения >= t_star (относительно начала + 2 секунды)
        let mut l_k_index = None;
        for (j, (time, _)) in thread_measurements.iter().enumerate() {
            if *time >= (min_start_time + skip_time_ns + t_star) {
                l_k_index = Some(j);
                break;
            }
        }

        // Если не нашли измерение >= t_star, используем последнее
        let l_k = l_k_index.unwrap_or(thread_measurements.len() - 1);

        // Интерполяция согласно RMBT спецификации
        let b_k = if l_k == 0 {
            // Если первое измерение уже >= t_star, интерполируем от начала
            interpolate_bytes_at_time(&thread_measurements, min_start_time + skip_time_ns + t_star)
        } else if l_k < thread_measurements.len() {
            // Интерполяция между двумя точками
            let (t_lk_minus_1, b_lk_minus_1) = thread_measurements[l_k - 1];
            let (t_lk, b_lk) = thread_measurements[l_k];

            if t_lk > t_lk_minus_1 {
                // b_k = b_k^(l_k-1) + (t* - t_k^(l_k-1)) * (b_k^(l_k) - b_k^(l_k-1)) / (t_k^(l_k) - t_k^(l_k-1))
                let target_time = min_start_time + skip_time_ns + t_star;
                let ratio = (target_time - t_lk_minus_1) as f64 / (t_lk - t_lk_minus_1) as f64;
                b_lk_minus_1 as f64 + ratio * (b_lk - b_lk_minus_1) as f64
            } else {
                // Если времена равны, используем последнее значение
                b_lk as f64
            }
        } else {
            // Если l_k указывает за пределы массива, используем последнее измерение
            thread_measurements.last().unwrap().1 as f64
        };

        // Вычитаем байты на начало (после пропуска 2 секунд)
        let b_k_adjusted = b_k - bytes_at_start;
        total_bytes += b_k_adjusted;
    }

    // Вычисляем скорость R = (1/t*) * Σ(b_k) с учетом пропуска первых 2 секунд
    let speed_bps = (total_bytes * 8.0) / (t_star as f64 / 1_000_000_000.0);
    let speed_gbps = speed_bps / 1_000_000_000.0;
    let speed_mbps = speed_bps / 1_000_000.0;

    (speed_bps, speed_gbps, speed_mbps)
}

fn interpolate_bytes_at_time(measurements: &[(u64, u64)], target_time: u64) -> f64 {
    if measurements.is_empty() {
        return 0.0;
    }

    let mut before = None;
    let mut after = None;

    for (time, bytes) in measurements {
        if *time <= target_time {
            before = Some((*time, *bytes));
        }
        if *time >= target_time {
            after = Some((*time, *bytes));
            break;
        }
    }

    match (before, after) {
        (Some((t0, b0)), Some((t1, b1))) if t1 > t0 => {
            let dt = t1 - t0;
            let db = b1 - b0;
            let dt_target = target_time - t0;
            b0 as f64 + (dt_target as f64 / dt as f64) * db as f64
        }
        (Some((_, b)), None) => b as f64,
        (None, Some((_, b))) => b as f64,
        _ => 0.0,
    }
}


pub fn calculate_download_speed_from_stats_silent(stats: &Vec<Vec<(u64, u64)>>) -> (f64, f64, f64) {
    calculate_speed_from_measurements(stats.clone())
}

pub fn calculate_upload_speed_from_stats_silent(stats: &Vec<Vec<(u64, u64)>>) -> (f64, f64, f64) {
    calculate_speed_from_measurements(stats.clone())
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