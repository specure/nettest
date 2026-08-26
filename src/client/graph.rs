//! Aggregating per-thread samples into the speed curve a result page draws.
//!
//! Port of `SpeedItems.java` from the control server, which is what turns a
//! measurement into `measurement_graphs.down_points` / `up_points`: at every
//! 100 ms step, each thread's cumulative byte count is interpolated to that
//! instant and the threads are summed.
//!
//! Two deliberate differences from the Java, both because it assumes threads
//! report in lockstep:
//!
//! * it truncates every thread to the *shortest* thread's sample count, which
//!   throws away the tail of every other thread — real threads deliver wildly
//!   different numbers of samples;
//! * it ends the curve at the *earliest* thread's last sample, so one lagging
//!   thread stalls the whole curve.
//!
//! Here every thread keeps its full series and the curve runs to the latest
//! sample; a thread that has already finished holds its final byte count for
//! the remaining steps. At the end of a phase — the only time this is used for
//! a stored result — every thread has reported, so the two agree.

/// One point of the curve.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GraphPoint {
    /// Bytes transferred across all threads by this instant.
    pub bytes_total: u64,
    /// Milliseconds since the phase started.
    pub time_elapsed_ms: u64,
    /// Speed at this point in bit/s.
    pub speed_bps: u64,
}

/// Distance between points, matching the control server's grid.
const STEP_NS: u64 = 100_000_000;

/// Build the curve from each thread's `(time_ns, cumulative_bytes)` samples.
pub fn speed_curve(per_thread: &[Vec<(u64, u64)>]) -> Vec<GraphPoint> {
    let target_time = per_thread
        .iter()
        .filter_map(|thread| thread.last().map(|(time, _)| *time))
        .max()
        .unwrap_or(0);
    if target_time == 0 {
        return Vec::new();
    }

    let mut points = Vec::new();
    let mut step = 0;
    while step < target_time {
        points.push(point_at(per_thread, step));
        step += STEP_NS;
    }
    // The final instant is what carries the phase's total, so it is always a
    // point of its own however the step lands.
    points.push(point_at(per_thread, target_time));
    points
}

/// Total bytes across threads at `target_time`, interpolated within whichever
/// pair of samples brackets it.
fn point_at(per_thread: &[Vec<(u64, u64)>], target_time: u64) -> GraphPoint {
    let mut bytes_total = 0u64;

    for thread in per_thread {
        if thread.is_empty() {
            continue;
        }
        match thread.iter().position(|(time, _)| *time >= target_time) {
            // Past this thread's last sample: it holds what it had. A thread
            // that finished early must not vanish from the total.
            None => bytes_total += thread[thread.len() - 1].1,
            Some(index) => {
                let (time_before, bytes_before) = if index == 0 { (0, 0) } else { thread[index - 1] };
                let (time_at, bytes_at) = thread[index];
                let span = time_at.saturating_sub(time_before);
                bytes_total += if span == 0 {
                    bytes_before
                } else {
                    let factor = (target_time - time_before) as f64 / span as f64;
                    let grown = ((bytes_at.saturating_sub(bytes_before)) as f64 * factor).round();
                    bytes_before + grown.max(0.0) as u64
                };
            }
        }
    }

    let time_elapsed_ms = (target_time as f64 / 1e6).round() as u64;
    let speed_bps = if time_elapsed_ms > 0 {
        ((bytes_total as f64 / time_elapsed_ms as f64) * 8.0 * 1000.0).round() as u64
    } else {
        0
    };

    GraphPoint {
        bytes_total,
        time_elapsed_ms,
        speed_bps,
    }
}

#[cfg(test)]
mod tests {
    use super::{speed_curve, GraphPoint};

    /// A thread transferring `bytes_total` at a constant rate until `until_ns`,
    /// sampled every 250 ms.
    fn thread(bytes_total: u64, until_ns: u64) -> Vec<(u64, u64)> {
        let step = 250_000_000u64;
        (1..=(until_ns / step))
            .map(|i| (i * step, bytes_total * (i * step) / until_ns))
            .collect()
    }

    #[test]
    fn sums_the_threads_at_every_step() {
        let second = 1_000_000_000u64;
        let curve = speed_curve(&[thread(10_000_000, 2 * second), thread(10_000_000, 2 * second)]);

        // 100 ms grid over two seconds, plus the closing point.
        assert_eq!(curve.len(), 21);
        let last = curve.last().unwrap();
        assert_eq!(last.time_elapsed_ms, 2000);
        assert_eq!(last.bytes_total, 20_000_000);
        // 20 MB in 2 s is 80 Mbit/s.
        assert_eq!(last.speed_bps, 80_000_000);
    }

    #[test]
    fn the_curve_only_grows() {
        let second = 1_000_000_000u64;
        let curve = speed_curve(&[thread(9_000_000, 7 * second), thread(4_000_000, 7 * second)]);
        for pair in curve.windows(2) {
            assert!(
                pair[1].bytes_total >= pair[0].bytes_total,
                "curve went backwards: {:?} then {:?}",
                pair[0],
                pair[1]
            );
        }
    }

    #[test]
    fn a_thread_that_finished_early_keeps_counting() {
        let second = 1_000_000_000u64;
        // One thread stops at 1 s having moved 1 MB, the other runs to 3 s.
        let curve = speed_curve(&[thread(1_000_000, second), thread(6_000_000, 3 * second)]);
        let last = curve.last().unwrap();
        assert_eq!(last.time_elapsed_ms, 3000);
        // Its megabyte is still part of the total — truncating the curve to the
        // shortest thread would have dropped two thirds of the transfer.
        assert_eq!(last.bytes_total, 7_000_000);
    }

    #[test]
    fn nothing_measured_is_no_curve() {
        assert!(speed_curve(&[]).is_empty());
        assert!(speed_curve(&[vec![], vec![]]).is_empty());
    }

    #[test]
    fn interpolates_between_samples() {
        // A single thread with one sample per second: at 500 ms the curve must
        // sit halfway, not at zero and not at the full second's value.
        let curve = speed_curve(&[vec![(1_000_000_000, 1_000_000), (2_000_000_000, 2_000_000)]]);
        let at_500ms = curve.iter().find(|p| p.time_elapsed_ms == 500).unwrap();
        assert_eq!(
            *at_500ms,
            GraphPoint { bytes_total: 500_000, time_elapsed_ms: 500, speed_bps: 8_000_000 }
        );
    }
}
