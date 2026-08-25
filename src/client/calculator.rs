#[cfg(not(target_arch = "wasm32"))]
use crate::client::client::Measurement;

pub fn calculate_speed_from_measurements(measurements: Vec<Vec<(u64, u64)>>) -> (f64, f64, f64) {
    if measurements.is_empty() {
        return (0.0, 0.0, 0.0);
    }

    let skip_time_ns = 1_000_000_000u64; // 1 second in nanoseconds
    
    // Find minimum measurement start time
    let min_start_time = measurements
        .iter()
        .filter_map(|m| m.first().map(|(time, _)| *time))
        .min()
        .unwrap_or(0);

    // Find t* - minimum time of last measurement among all threads
    let t_star_original = measurements
        .iter()
        .filter_map(|m| m.last().map(|(time, _)| *time))
        .min()
        .unwrap_or(0);

    if t_star_original == 0 {
        return (0.0, 0.0, 0.0);
    }

    // t* accounting for skipping first 2 seconds
    let t_star = t_star_original - skip_time_ns;
    // If after skipping 2 seconds time is insufficient, return 0
    if t_star <= 0 {
        return (0.0, 0.0, 0.0);
    }

    let mut total_bytes = 0.0;

    // For each thread k
    for thread_measurements in measurements {
        if thread_measurements.is_empty() {
            continue;
        }


        // Interpolate data at start (after skipping 2 seconds)
        let bytes_at_start = interpolate_bytes_at_time(&thread_measurements, min_start_time + skip_time_ns);
        
        // Find l_k - index of first measurement >= t_star (relative to start + 2 seconds)
        let mut l_k_index = None;
        for (j, (time, _)) in thread_measurements.iter().enumerate() {
            if *time >= (min_start_time + skip_time_ns + t_star) {
                l_k_index = Some(j);
                break;
            }
        }

        // If no measurement >= t_star found, use last one
        let l_k = l_k_index.unwrap_or(thread_measurements.len() - 1);

        // Interpolation according to RMBT specification
        let b_k = if l_k == 0 {
            // If first measurement already >= t_star, interpolate from start
            interpolate_bytes_at_time(&thread_measurements, min_start_time + skip_time_ns + t_star)
        } else if l_k < thread_measurements.len() {

            // Interpolation between two points
            let (t_lk_minus_1, b_lk_minus_1) = thread_measurements[l_k - 1];
            let (t_lk, b_lk) = thread_measurements[l_k];

            if t_lk > t_lk_minus_1 {
                // b_k = b_k^(l_k-1) + (t* - t_k^(l_k-1)) * (b_k^(l_k) - b_k^(l_k-1)) / (t_k^(l_k) - t_k^(l_k-1))
                let target_time = min_start_time + skip_time_ns + t_star;
                let ratio = (target_time - t_lk_minus_1) as f64 / (t_lk - t_lk_minus_1) as f64;
                b_lk_minus_1 as f64 + ratio * (b_lk - b_lk_minus_1) as f64
            } else {
                // If times are equal, use last value
                b_lk as f64
            }
        } else {
            // If l_k points beyond array bounds, use last measurement
            thread_measurements.last().unwrap().1 as f64
        };

        // Subtract bytes at start (after skipping 2 seconds)
        let b_k_adjusted = b_k - bytes_at_start;
        total_bytes += b_k_adjusted;
    }

    // Calculate speed R = (1/t*) * Σ(b_k) accounting for skipping first 2 seconds
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

#[cfg(not(target_arch = "wasm32"))]
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
/// Interarrival jitter over a series of latency samples, per RFC 3550 §6.4.1:
///
/// ```text
/// J(i) = J(i-1) + (|D(i-1,i)| - J(i-1)) / 16
/// ```
///
/// RFC 3550 feeds `D` with *one-way* transit-time differences, measured on a
/// stream of equally spaced packets. Here `samples` are round-trip latencies
/// (the RMBT `PING`/`TIME` samples), so this measures the variation of the round
/// trip: an approximation that shares the RFC's smoothing, not its packet model.
/// Over TCP it is also blind to retransmissions, which show up as latency spikes
/// rather than loss — callers should label it accordingly and must not report a
/// packet-loss figure derived from it.
///
/// Returns `None` for fewer than two samples (no difference to measure).
pub fn rfc3550_jitter_ns(samples: &[u64]) -> Option<f64> {
    if samples.len() < 2 {
        return None;
    }
    let mut jitter = 0.0f64;
    for pair in samples.windows(2) {
        let d = (pair[1] as f64) - (pair[0] as f64);
        jitter += (d.abs() - jitter) / 16.0;
    }
    Some(jitter)
}

/// The same estimator over *transit times* — arrival minus the sender's own
/// timestamp, as RFC 3550's `D(i,j)` intends.
///
/// Signed on purpose. Two machines rarely agree on the wall clock, so a transit
/// time is the real one plus a constant offset that can easily be seconds and
/// can point either way. The offset cancels in the consecutive differences, so
/// unsynchronised clocks are fine — but only if the sign survives: clamping a
/// negative transit to zero flattens the whole series and reports a jitter of
/// exactly 0.
pub fn rfc3550_jitter_from_transits_ns(transits: &[i64]) -> Option<f64> {
    if transits.len() < 2 {
        return None;
    }
    let mut jitter = 0.0f64;
    for pair in transits.windows(2) {
        let d = (pair[1] - pair[0]) as f64;
        jitter += (d.abs() - jitter) / 16.0;
    }
    Some(jitter)
}

#[cfg(test)]
mod tests {
    use super::{rfc3550_jitter_from_transits_ns, rfc3550_jitter_ns};

    /// A clock offset between the two machines — of either sign, and far larger
    /// than the jitter being measured — must not reach the result.
    #[test]
    fn transit_jitter_ignores_the_clock_offset() {
        let real: Vec<i64> = vec![20_000_000, 21_000_000, 19_500_000, 20_500_000, 20_100_000];
        let expected = rfc3550_jitter_from_transits_ns(&real).unwrap();
        for offset in [-5_000_000_000i64, -1_330_000_000, 1_330_000_000, 5_000_000_000] {
            let shifted: Vec<i64> = real.iter().map(|t| t + offset).collect();
            let got = rfc3550_jitter_from_transits_ns(&shifted).unwrap();
            assert!(
                (got - expected).abs() < 1.0,
                "offset {offset} changed the jitter: {got} vs {expected}"
            );
        }
    }

    #[test]
    fn transit_jitter_needs_two_samples() {
        assert_eq!(rfc3550_jitter_from_transits_ns(&[]), None);
        assert_eq!(rfc3550_jitter_from_transits_ns(&[-42]), None);
    }

    #[test]
    fn needs_at_least_two_samples() {
        assert_eq!(rfc3550_jitter_ns(&[]), None);
        assert_eq!(rfc3550_jitter_ns(&[1_000_000]), None);
    }

    #[test]
    fn constant_latency_has_no_jitter() {
        let samples = [5_000_000u64; 50];
        assert_eq!(rfc3550_jitter_ns(&samples), Some(0.0));
    }

    #[test]
    fn converges_towards_the_mean_absolute_difference() {
        // Alternating 10 ms / 20 ms: every |D| is 10 ms, so the smoothed jitter
        // must approach 10 ms from below and never exceed it.
        let samples: Vec<u64> = (0..200)
            .map(|i| if i % 2 == 0 { 10_000_000 } else { 20_000_000 })
            .collect();
        let j = rfc3550_jitter_ns(&samples).unwrap();
        assert!(j > 9_900_000.0 && j <= 10_000_000.0, "jitter was {j}");
    }

    #[test]
    fn one_spike_decays_and_does_not_dominate() {
        // A single 100 ms outlier in an otherwise steady 10 ms stream: the 1/16
        // smoothing must keep the result far below the spike.
        let mut samples = vec![10_000_000u64; 100];
        samples[50] = 110_000_000;
        let j = rfc3550_jitter_ns(&samples).unwrap();
        assert!(j < 5_000_000.0, "single spike dominated the result: {j}");
    }
}
