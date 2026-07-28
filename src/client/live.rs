//! Live measurement state shared with external consumers (e.g. the Flutter FFI
//! layer). The runner updates this snapshot as the measurement progresses so a
//! caller can poll it (see `run_measurement_with_progress`).

use std::sync::{Arc, Mutex};

/// Per-thread `(time_ns, bytes)` samples, published live during a phase.
pub type ThreadSamples = Arc<Mutex<Vec<(u64, u64)>>>;

/// Per-thread ping-phase progress (0-100), published live while the (short,
/// ~1s) ping phase is running so the UI can show an in-progress percentage
/// instead of nothing until the phase's final result is ready.
pub type PingProgress = Arc<Mutex<Option<f64>>>;

/// Live sample sink handed to a single measurement thread. The thread copies
/// its current download/upload samples here periodically so a caller can draw
/// the graph while the test is still running.
#[derive(Clone, Debug)]
pub struct LiveSink {
    pub download: ThreadSamples,
    pub upload: ThreadSamples,
    pub ping_progress: PingProgress,
}

impl LiveSink {
    pub fn new() -> Self {
        LiveSink {
            download: Arc::new(Mutex::new(Vec::new())),
            upload: Arc::new(Mutex::new(Vec::new())),
            ping_progress: Arc::new(Mutex::new(None)),
        }
    }
}

impl Default for LiveSink {
    fn default() -> Self {
        Self::new()
    }
}

/// Coarse measurement phase, surfaced to external callers.
///
/// Order roughly follows the test pipeline:
/// `greeting` → `init` → `ping` → `jitter` → `packetloss` → `download` →
/// `upload` → `done`.
#[derive(Clone, Default)]
pub struct LiveState {
    pub phase: String,
    pub ping_ms: Option<f64>,
    pub download_mbps: Option<f64>,
    pub upload_mbps: Option<f64>,
    pub jitter_ms: Option<f64>,
    pub packet_loss_percent: Option<f64>,
    pub done: bool,
    /// Per-thread live download samples (one entry per thread).
    pub download_threads: Vec<ThreadSamples>,
    /// Per-thread live upload samples (one entry per thread).
    pub upload_threads: Vec<ThreadSamples>,
    /// Per-thread live ping-phase progress (one entry per thread; only the
    /// thread actually running ping, normally thread 0, ever sets it).
    pub ping_progress_threads: Vec<PingProgress>,
}

/// Thread-safe handle to the live state.
pub type SharedLive = Arc<Mutex<LiveState>>;

/// Create a fresh shared live-state handle in the `idle` phase.
pub fn new_shared_live() -> SharedLive {
    Arc::new(Mutex::new(LiveState {
        phase: "idle".to_string(),
        ..Default::default()
    }))
}

/// Set the current phase, ignoring lock poisoning.
pub fn set_phase(live: &Option<SharedLive>, phase: &str) {
    if let Some(live) = live {
        if let Ok(mut guard) = live.lock() {
            guard.phase = phase.to_string();
        }
    }
}

/// Apply an arbitrary mutation to the live state, ignoring lock poisoning.
pub fn update<F: FnOnce(&mut LiveState)>(live: &Option<SharedLive>, f: F) {
    if let Some(live) = live {
        if let Ok(mut guard) = live.lock() {
            f(&mut guard);
        }
    }
}
