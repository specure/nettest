use std::collections::HashMap;
use std::time::{Duration, Instant};
use log::debug;
use plotters::prelude::*;
use plotters::style::*;
use textplots::{Chart, Shape};
use textplots::Plot;
use rgb::RGB8;

/// Структура для хранения результатов измерений
#[derive(Debug, Clone)]
pub struct MeasurementResult {
    pub thread_id: usize,
    pub measurements: Vec<(u64, u64)>, // (time_ns, bytes)
}

/// Сервис для отрисовки графиков
pub struct GraphService;

impl GraphService {
    /// Отрисовывает график загрузки
    pub fn print_download(measurement_results: &[MeasurementResult], speed_data: &(f64, f64, f64)) {
        println!("\n=== DOWNLOAD SPEED GRAPH ===");
        Self::print_speed_graph(measurement_results, "Download");
    }

    /// Отрисовывает график выгрузки
    pub fn print_upload(measurement_results: &[MeasurementResult], speed_data: &(f64, f64, f64)) {
        println!("\n=== UPLOAD SPEED GRAPH ===");
        Self::print_speed_graph(measurement_results, "Upload");
    }

    /// Внутренний метод для отрисовки графика скорости
    fn print_speed_graph(measurement_results: &[MeasurementResult], test_type: &str) {
        if measurement_results.is_empty() {
            debug!("No data to display");
            return;
        }

        // Debug: print all raw measurement_results
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

        // Calculate speeds for each 0.1s
        let speed_data = Self::calculate_speeds_per_second(measurement_results, min_time, max_time);
        if speed_data.is_empty() {
            println!("No speed data");
            return;
        }

        // Debug: print all speed_data
        debug!("[DEBUG] speed_data (second, Mbit/s, bytes):");
        for (s, mbps, bytes) in &speed_data {
            debug!("  second {:.1}: {:.2} Mbit/s, {} bytes", s, mbps, bytes);
        }

        // Используем все значения для графика
        let plot_data: Vec<(f64, f64)> = speed_data.iter().map(|(s, mbps, _)| (*s, *mbps)).collect();
        // Debug: print plot_data
        debug!("[DEBUG] plot_data (second, Mbit/s):");
        for (s, mbps) in &plot_data {
            debug!("  second {:.1}: {:.2} Mbit/s", s, mbps);
        }
        Self::print_speed_textplot(&plot_data, test_type);
    }

    /// Получает общий временной диапазон из всех измерений
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

    /// Вычисляет скорость для каждой секунды
    fn calculate_speeds_per_second(
        measurement_results: &[MeasurementResult], 
        min_time: u64, 
        max_time: u64
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
                
                // Найти две точки для интерполяции: до и после target_time
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
                
                // Интерполяция между двумя точками
                let bytes = match (before, after) {
                    (Some((t0, b0)), Some((t1, b1))) if t1 > t0 => {
                        // Линейная интерполяция: b = b0 + (t - t0) * (b1 - b0) / (t1 - t0)
                        let dt = t1 - t0;
                        let db = b1 - b0;
                        let dt_target = target_time - t0;
                        b0 as f64 + (dt_target as f64 / dt as f64) * db as f64
                    }
                    (Some((_, b)), None) => {
                        // Если нет точки после target_time, используем последнюю известную
                        b as f64
                    }
                    (None, Some((_, b))) => {
                        0.0 as f64
                    }
                    _ => 0.0,
                };

                debug!("bytes: {}", bytes);
                
                total_bytes += bytes;
            }
            
            let speed_mbit = if t > 0.0  {
                total_bytes * 8.0 / t / 1_000_000.0
            } else {
                0.0
            };
            debug!("t: {}, speed_mbit: {}", t, speed_mbit);
            speed_data.push((t, speed_mbit, total_bytes as u64));
        }
        speed_data
    }

    /// Рисует график скорости через textplots
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

        println!("{} Test - Speed over Time (textplots)", test_type);
        Chart::new(192, 48, min_x, max_x)
            .lineplot(&Shape::Lines(&points))
            .display();
        println!("X axis: Seconds, Y axis: Mbit/s");
    }
} 