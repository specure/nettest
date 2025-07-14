use crate::client::args_parser::parse_args;
use crate::client::print::graph_service::{GraphService};
use crate::client::print::printer::{print_help, print_test_header};
use crate::client::runnner::run_threads;
use crate::config::Config;
use log::{debug, info, LevelFilter};
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
    pub thread_count: usize,
    pub log: Option<LevelFilter>,
    pub server: String,
    pub port: u16,
    pub tls_port: u16,
}

pub async fn client_run(args: Vec<String>, dafault_config: Config) -> anyhow::Result<()> {
    info!("Starting measurement client...");

    print_test_header();

    if args.contains(&"-h".to_string()) || args.contains(&"--help".to_string()) {
        print_help();
    }

    let config = parse_args(args, dafault_config)?;


    let stats: Arc<Mutex<SharedStats>> = Arc::new(Mutex::new(SharedStats::default()));

    let state_refs = run_threads(config.clone(), stats)?;


    if config.graphs {
        GraphService::print_graph(&state_refs);
    }
    Ok(())
}

