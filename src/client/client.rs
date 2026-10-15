use crate::client::api::run_measurement_with_chunk_size;
use crate::client::args_parser::{parse_args, print_help};
use crate::client::constants::init_max_chunk_size;
use crate::client::print::graph_service::GraphService;
use crate::client::print::json::print_json_result;
use crate::client::print::printer::print_test_header;
use crate::client::runnner::run_threads;
use crate::config::FileConfig;
use crate::voip::RtpQoSResult;
use crate::udp::UdpQoSResult;
use log::{info, LevelFilter};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use crate::client::state::TestPhase;

pub struct CommandLineArgs {
    pub thread_count: usize,
    pub addr: SocketAddr,
    pub use_tls: bool,
    pub use_websocket: bool,
}

#[derive(Clone)]
pub struct Measurement {
    pub measurements: Vec<(u64, u64)>,
    pub failed: bool,
    pub thread_id: usize,
    pub phase: TestPhase,
    pub upload_measurements: Vec<(u64, u64)>,
    pub envelope: Option<String>,
    pub ping_median_ns: Option<u64>,
    pub voip_result_in: Option<RtpQoSResult>,
    pub voip_result_out: Option<RtpQoSResult>,
    pub udp_result_out: Option<UdpQoSResult>,
    pub udp_result_in: Option<UdpQoSResult>,
}

#[derive(Default)]
pub struct SharedStats {
    pub download_measurements: Vec<Vec<(u64, u64)>>,
    pub upload_measurements: Vec<Vec<(u64, u64)>>,
}

#[derive(Clone, Debug)]
pub struct ClientConfig {
    pub use_tls: bool,
    pub use_websocket: bool,
    pub graphs: bool,
    pub raw_output: bool,
    pub json_output: bool,
    pub thread_count: usize,
    pub log: Option<LevelFilter>,
    pub server: Option<String>,
    pub port: u16,
    pub tls_port: u16,
    pub udp_port: u16,
    pub x_nettest_client: String,
    pub control_server: String,
    pub save_results: bool,
    pub signed_result: bool,
    pub client_uuid: Option<String>,
    pub git_hash: Option<String>,
    pub legacy: bool,
    /// PUTTIMERESULT interim reporting interval in ms (0 = only final result).
    pub put_time_result_interval_ms: u64,
    /// Per-phase durations in ms (configurable from the client).
    pub download_duration_ms: u64,
    pub upload_duration_ms: u64,
    pub jitter_duration_ms: u64,
    pub packetloss_duration_ms: u64,
    /// Which phases run. The CLI runs all of them; the server's own internet
    /// measurement splits speed (download/upload) and quality (ping, jitter,
    /// packet loss) into two runs so the page can show the speeds sooner.
    pub phases: Phases,
}

/// Phase selection for one run. The pre-test runs only with a transfer phase.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Phases {
    pub ping: bool,
    /// Jitter (VoIP) and packet loss (UDP); also skipped in legacy mode.
    pub qos: bool,
    pub download: bool,
    pub upload: bool,
}

impl Default for Phases {
    fn default() -> Self {
        Phases { ping: true, qos: true, download: true, upload: true }
    }
}

impl Phases {
    /// Download and upload only.
    pub fn speed() -> Self {
        Phases { ping: false, qos: false, download: true, upload: true }
    }
    /// Ping, jitter and packet loss only.
    pub fn quality() -> Self {
        Phases { ping: true, qos: true, download: false, upload: false }
    }
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            use_tls: false,
            use_websocket: false,
            graphs: false,
            raw_output: false,
            json_output: false,
            thread_count: 3,
            log: None,
            server: None,
            port: 5005,
            tls_port: 443,
            udp_port: 5004,
            x_nettest_client: "nt".to_string(),
            control_server: "https://api.nettest.org".to_string(),
            save_results: false,
            signed_result: false,
            client_uuid: None,
            git_hash: None,
            legacy: false,
            put_time_result_interval_ms: 0,
            download_duration_ms: 7000,
            upload_duration_ms: 7000,
            jitter_duration_ms: 4000,
            packetloss_duration_ms: 4000,
            phases: Phases::default(),
        }
    }
}

pub async fn client_run(args: Vec<String>, dafault_config: FileConfig) -> anyhow::Result<()> {
    info!("Starting measurement client...");

    // Initialize MAX_CHUNK_SIZE from config
    init_max_chunk_size(dafault_config.max_chunk_size);
    let max_chunk_size = dafault_config.max_chunk_size;

    if args.contains(&"-h".to_string()) || args.contains(&"--help".to_string()) {
        print_help();
        return Ok(());
    }

    let config = parse_args(args, dafault_config).await?;

    if config.json_output {
        let result = run_measurement_with_chunk_size(config.clone(), max_chunk_size).await?;
        print_json_result(&result, &config);
        return Ok(());
    }

    if !config.raw_output {
        print_test_header();
    }

    let stats: Arc<Mutex<SharedStats>> = Arc::new(Mutex::new(SharedStats::default()));

    info!("Config: {:?}", config);

    let state_refs = run_threads(config.clone(), stats, None).await;

    if config.graphs {
        GraphService::print_graph(&state_refs.unwrap());
    }
    Ok(())
}
