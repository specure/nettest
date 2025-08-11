use crate::client::args_parser::{parse_args, print_help};
use crate::client::print::graph_service::GraphService;
use crate::client::print::printer::print_test_header;
use crate::client::runnner::run_threads;
use crate::config::FileConfig;
use log::{info, LevelFilter};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

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
    pub upload_measurements: Vec<(u64, u64)>,
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
    pub x_nettest_client: String,
    pub control_server: String,
    pub save_results: bool,
    pub client_uuid: Option<String>,
}

pub async fn client_run(args: Vec<String>, dafault_config: FileConfig) -> anyhow::Result<()> {
    info!("Starting measurement client...");

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

    let state_refs = run_threads(config.clone(), stats).await;

    if config.graphs {
        GraphService::print_graph(&state_refs.unwrap());
    }
    Ok(())
}
