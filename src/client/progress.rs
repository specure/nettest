use serde::{Serialize, Deserialize};
use crate::client::state::TestPhase;
use std::sync::Arc;
use std::sync::Mutex;

/// Thread information for UI display
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadInfo {
    pub thread_id: usize,
    pub phase: String,
    pub bytes_received: u64,
    pub bytes_sent: u64,
    pub failed: bool,
    pub download_measurements: Vec<(u64, u64)>, // (time_ns, bytes) - last poll results
    pub upload_measurements: Vec<(u64, u64)>,   // (time_ns, bytes) - last poll results
}

/// Detailed thread result after test completion
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadResult {
    pub thread_id: usize,
    pub failed: bool,
    pub download_measurements: Vec<(u64, u64)>, // (time_ns, bytes)
    pub upload_measurements: Vec<(u64, u64)>,   // (time_ns, bytes)
    pub total_bytes_received: u64,
    pub total_bytes_sent: u64,
    pub download_samples: usize,
    pub upload_samples: usize,
    pub envelope: Option<String>,
}

/// Progress update sent to Flutter after each barrier.wait
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeasurementProgress {
    pub phase: String,
    pub ping_median_ms: Option<f64>,
    pub download_speed_mbps: Option<f64>,
    pub upload_speed_mbps: Option<f64>,
    pub download_measurements: Vec<Vec<(u64, u64)>>,
    pub upload_measurements: Vec<Vec<(u64, u64)>>,
    pub progress_percent: f64,
    pub bytes_received: u64,
    pub bytes_sent: u64,
    pub thread_count: usize,
    pub active_threads: usize,
    pub threads: Vec<ThreadInfo>, // Information about each thread
    pub thread_results: Vec<ThreadResult>, // Detailed results after completion
}

/// Thread state snapshot for progress reporting
#[derive(Debug, Clone)]
pub struct ThreadState {
    pub phase: TestPhase,
    pub bytes_received: u64,
    pub bytes_sent: u64,
    pub download_measurements: Vec<(u64, u64)>,
    pub upload_measurements: Vec<(u64, u64)>,
    pub failed: bool,
}

/// Shared state for all threads (for progress reporting)
pub type ThreadStates = Arc<Mutex<Vec<Option<ThreadState>>>>;

impl MeasurementProgress {
    pub fn from_phase(phase: &TestPhase) -> String {
        match phase {
            TestPhase::GreetingSendConnectionType | 
            TestPhase::GreetingSendToken | 
            TestPhase::GreetingReceiveGreeting | 
            TestPhase::GreetingReceiveResponse | 
            TestPhase::GreetingCompleted => "greeting".to_string(),
            
            TestPhase::GetChunksSendChunksCommand |
            TestPhase::GetChunksReceiveChunk |
            TestPhase::GetChunksSendOk |
            TestPhase::GetChunksReceiveTime |
            TestPhase::GetChunksCompleted => "init_download".to_string(),
            
            TestPhase::PingSendPing |
            TestPhase::PingReceivePong |
            TestPhase::PingSendOk |
            TestPhase::PingReceiveTime |
            TestPhase::PingCompleted => "ping".to_string(),
            
            TestPhase::GetTimeSendCommand |
            TestPhase::GetTimeReceiveChunk |
            TestPhase::GetTimeSendOk |
            TestPhase::GetTimeReceiveTime |
            TestPhase::GetTimeCompleted => "download".to_string(),
            
            TestPhase::PerfSendCommand |
            TestPhase::PerfReceiveOk |
            TestPhase::PerfSendChunks |
            TestPhase::PerfSendLastChunk |
            TestPhase::PerfReceiveTime |
            TestPhase::PerfCompleted => "upload".to_string(),
            
            TestPhase::SignedResultSend |
            TestPhase::SignedResultReceive |
            TestPhase::SignedResultSendOk |
            TestPhase::SignedResultCompleted => "signed_result".to_string(),
        }
    }
}

