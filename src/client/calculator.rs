#[cfg(not(target_arch = "wasm32"))]
use crate::client::client::Measurement;

pub fn calculate_speed_from_measurements(measurements: Vec<Vec<(u64, u64)>>) -> (f64, f64, f64) {
    if measurements.is_empty() {
        return (0.0, 0.0, 0.0);
    }

    // Skip the first second: TCP slow start is not the link's speed.
    let skip_time_ns = 1_000_000_000u64;

    // RMBT's t*: the earliest last-sample across threads, so the window covers
    // a stretch every thread actually took part in. Sample times are relative
    // to each thread's own phase start, which is the clock the window lives on
    // — offsetting it by the earliest *first* sample, as this used to do,
    // pushed the window past the end of every series.
    let window_end = measurements
        .iter()
        .filter_map(|m| m.last().map(|(time, _)| *time))
        .min()
        .unwrap_or(0);

    if window_end <= skip_time_ns {
        // Everything happened inside the skipped second: nothing to measure.
        return (0.0, 0.0, 0.0);
    }
    let duration_ns = window_end - skip_time_ns;

    // Bytes each thread moved inside the window. `interpolate_bytes_at_time`
    // clamps at both ends of a series, which is what keeps a thread that
    // stopped early from being extrapolated: reading "between the last two
    // samples" past the end of the data multiplied their difference by the
    // overshoot, and since the final two samples often share a timestamp, that
    // factor could reach four digits and invent tens of megabytes.
    let total_bytes: f64 = measurements
        .iter()
        .filter(|m| !m.is_empty())
        .map(|m| {
            let at_end = interpolate_bytes_at_time(m, window_end);
            let at_start = interpolate_bytes_at_time(m, skip_time_ns);
            (at_end - at_start).max(0.0)
        })
        .sum();

    let speed_bps = (total_bytes * 8.0) / (duration_ns as f64 / 1_000_000_000.0);
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
        // A target that lands exactly on a sample makes `before` and `after`
        // that same sample, and then there is nothing to interpolate — the
        // sample *is* the answer. Guarding this case with `t1 > t0` and letting
        // it fall through to the catch-all returned zero bytes instead, so the
        // bytes transferred before the skip window were never subtracted and
        // every speed computed from evenly spaced samples came out high. Server
        // `TIMERESULT` samples are evenly spaced, so the upload direction hit
        // this almost every time.
        (Some((t0, b0)), Some((t1, b1))) => {
            if t1 > t0 {
                let dt = t1 - t0;
                let db = b1 - b0;
                let dt_target = target_time - t0;
                b0 as f64 + (dt_target as f64 / dt as f64) * db as f64
            } else {
                b1 as f64
            }
        }
        (Some((_, b)), None) | (None, Some((_, b))) => b as f64,
        (None, None) => 0.0,
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
    use super::{
        calculate_speed_from_measurements, interpolate_bytes_at_time,
        rfc3550_jitter_from_transits_ns, rfc3550_jitter_ns,
    };

    /// A thread's samples: `bytes_total` transferred at a constant rate until
    /// `until_ns`, reported every 250 ms the way the server's `TIMERESULT` does.
    fn series(bytes_total: u64, until_ns: u64) -> Vec<(u64, u64)> {
        let step = 250_000_000u64;
        (1..=(until_ns / step))
            .map(|i| (i * step, bytes_total * (i * step) / until_ns))
            .collect()
    }

    #[test]
    fn interpolating_exactly_on_a_sample_returns_that_sample() {
        let samples = [(1_000_000_000u64, 5_000_000u64), (2_000_000_000, 9_000_000)];
        // The target lands on a sample: there is nothing to interpolate between,
        // and the answer is the sample itself — not zero, which is what a
        // "needs two distinct points" guard used to produce.
        assert_eq!(interpolate_bytes_at_time(&samples, 1_000_000_000), 5_000_000.0);
        assert_eq!(interpolate_bytes_at_time(&samples, 2_000_000_000), 9_000_000.0);
        // Between samples it still interpolates.
        assert_eq!(interpolate_bytes_at_time(&samples, 1_500_000_000), 7_000_000.0);
    }

    #[test]
    fn healthy_threads_report_the_actual_rate() {
        let second = 1_000_000_000u64;
        let threads = vec![
            series(9_000_000, 7 * second),
            series(9_000_000, 7 * second),
            series(9_000_000, 7 * second),
        ];
        // 27 MB in 7 s at a constant rate is 30.9 Mbit/s, and skipping the first
        // second must not change a constant rate.
        let (_, _, mbps) = calculate_speed_from_measurements(threads);
        assert!((mbps - 30.9).abs() < 0.5, "reported {mbps} Mbit/s for a 30.9 Mbit/s stream");
    }

    #[test]
    fn a_thread_that_stops_early_does_not_inflate_the_result() {
        // The shape that produced 830 Mbit/s against a real server: two threads
        // run the full phase while a third stops after 1.25 s, collapsing t* to
        // a quarter second. The rate must still describe the traffic.
        let second = 1_000_000_000u64;
        let threads = vec![
            series(30_000_000, 7 * second),
            series(30_000_000, 7 * second),
            series(400_000, 1_250_000_000),
        ];
        let (_, _, mbps) = calculate_speed_from_measurements(threads);
        assert!(mbps < 100.0, "a short thread inflated the result to {mbps} Mbit/s");
    }

    #[test]
    fn the_last_two_samples_sharing_a_timestamp_cannot_invent_bytes() {
        // Taken from a real run: dense samples, and the final pair a few
        // microseconds apart. Reading "between the last two samples" at a time
        // past the end of the series multiplied that pair's difference by the
        // overshoot — with a ratio above a thousand it conjured 69 MB out of a
        // 11 MB thread and reported 830 Mbit/s for an 80 Mbit/s link.
        let mut dense: Vec<(u64, u64)> = (1..=200)
            .map(|i| (i * 37_000_000u64, i * 57_000u64))
            .collect();
        let (last_t, last_b) = *dense.last().unwrap();
        dense.push((last_t + 10_000, last_b + 1_000));
        let threads = vec![dense.clone(), dense.clone(), dense];
        let (_, _, mbps) = calculate_speed_from_measurements(threads);
        // Three threads at ~1.54 MB/s each over the measured window.
        assert!(mbps < 60.0, "extrapolation inflated the result to {mbps} Mbit/s");
    }

    #[test]
    fn a_phase_shorter_than_the_skip_window_reports_nothing() {
        // Every sample falls inside the skipped first second: there is no window
        // left to measure. Must yield zero rather than underflow into a
        // ~584-year one.
        let threads = vec![series(400_000, 800_000_000)];
        assert_eq!(calculate_speed_from_measurements(threads), (0.0, 0.0, 0.0));
    }


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
