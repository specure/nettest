use std::sync::{Arc, Mutex};

use crate::client::calculator::{
    calculate_download_speed_from_stats_silent, calculate_upload_speed_from_stats_silent,
};
use crate::client::client::{ClientConfig, SharedStats};
use crate::client::constants::init_max_chunk_size;
use crate::client::live::SharedLive;
use crate::client::runnner::run_threads;

#[derive(Debug, Clone)]
pub struct MeasurementResult {
    pub ping_median_ns: Option<u64>,
    pub download_speed_bps: f64,
    pub download_speed_gbps: f64,
    pub upload_speed_bps: f64,
    pub upload_speed_gbps: f64,
    pub jitter_ns: Option<u64>,
    pub packet_loss_percent: Option<f32>,
    pub num_threads: usize,
    pub failed_threads: usize,
}

pub async fn run_measurement(config: ClientConfig) -> anyhow::Result<MeasurementResult> {
    let stats: Arc<Mutex<SharedStats>> = Arc::new(Mutex::new(SharedStats::default()));
    run_measurement_inner(config, stats, None).await
}

/// Like [`run_measurement`] but reports progress into the supplied shared
/// `live` state and exposes the shared `stats` so a caller can derive live
/// graphs from the raw per-thread measurements while the test runs.
pub async fn run_measurement_with_progress(
    config: ClientConfig,
    stats: Arc<Mutex<SharedStats>>,
    live: SharedLive,
) -> anyhow::Result<MeasurementResult> {
    let result = run_measurement_inner(config, stats, Some(live.clone())).await;
    if let Ok(mut guard) = live.lock() {
        guard.phase = "done".to_string();
        guard.done = true;
    }
    result
}

async fn run_measurement_inner(
    config: ClientConfig,
    stats: Arc<Mutex<SharedStats>>,
    live: Option<SharedLive>,
) -> anyhow::Result<MeasurementResult> {
    init_max_chunk_size(None);

    let thread_count = config.thread_count;

    let measurements = run_threads(config, stats.clone(), live).await?;

    let stats_guard = stats.lock().unwrap();
    let (dl_bps, dl_gbps, _) =
        calculate_download_speed_from_stats_silent(&stats_guard.download_measurements);
    let (ul_bps, ul_gbps, _) =
        calculate_upload_speed_from_stats_silent(&stats_guard.upload_measurements);
    drop(stats_guard);

    let failed_threads = thread_count - measurements.len();

    let thread_0 = measurements.iter().find(|m| m.thread_id == 0);

    let ping_median_ns = thread_0.and_then(|m| m.ping_median_ns);

    let jitter_ns = thread_0.and_then(|m| {
        match (&m.voip_result_in, &m.voip_result_out) {
            (Some(i), Some(o)) => Some(i.mean_jitter.max(o.mean_jitter) as u64),
            (Some(i), None) => Some(i.mean_jitter as u64),
            (None, Some(o)) => Some(o.mean_jitter as u64),
            (None, None) => None,
        }
    });

    let packet_loss_percent = thread_0.and_then(|m| {
        match (&m.udp_result_out, &m.udp_result_in) {
            (Some(o), Some(i)) => Some(o.packet_loss_rate.max(i.packet_loss_rate) as f32),
            (Some(o), None) => Some(o.packet_loss_rate as f32),
            (None, Some(i)) => Some(i.packet_loss_rate as f32),
            (None, None) => None,
        }
    });

    Ok(MeasurementResult {
        ping_median_ns,
        download_speed_bps: dl_bps,
        download_speed_gbps: dl_gbps,
        upload_speed_bps: ul_bps,
        upload_speed_gbps: ul_gbps,
        jitter_ns,
        packet_loss_percent,
        num_threads: thread_count,
        failed_threads,
    })
}
