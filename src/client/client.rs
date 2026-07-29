use crate::client::args_parser::{parse_args, print_help};
use crate::client::constants::init_max_chunk_size;
use crate::client::print::graph_service::GraphService;
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
    /// Whether to run the jitter (VoIP) and packet loss (UDP) phases at all.
    pub run_jitter_and_packetloss: bool,
    /// PUTTIMERESULT interim reporting interval in ms (0 = only final result).
    pub put_time_result_interval_ms: u64,
    /// Per-phase durations in ms (configurable from the client).
    pub download_duration_ms: u64,
    pub upload_duration_ms: u64,
    pub jitter_duration_ms: u64,
    pub packetloss_duration_ms: u64,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            use_tls: false,
            use_websocket: false,
            graphs: false,
            raw_output: false,
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
            run_jitter_and_packetloss: true,
            put_time_result_interval_ms: 0,
            download_duration_ms: 7000,
            upload_duration_ms: 7000,
            jitter_duration_ms: 4000,
            packetloss_duration_ms: 4000,
        }
    }
}

pub async fn client_run(args: Vec<String>, dafault_config: FileConfig) -> anyhow::Result<()> {
    info!("Starting measurement client...");

    // Initialize MAX_CHUNK_SIZE from config
    init_max_chunk_size(dafault_config.max_chunk_size);

    if args.contains(&"-h".to_string()) || args.contains(&"--help".to_string()) {
        print_help();
        return Ok(());
    }

    let config = parse_args(args, dafault_config).await?;

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
