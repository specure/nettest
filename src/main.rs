use std::error::Error;
use log::{error, info, debug};
use fastrand::Rng;
use crate::server::server::Server;
use crate::server::server_config::ServerConfig;

pub mod server;
pub mod handlers;
pub mod utils;
pub mod config;
pub mod logger;
pub mod stream;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let config = match ServerConfig::from_args() {
        Ok(config) => config,
        Err(e) => {
            eprintln!("Failed to parse configuration: {}", e);
            return Err(e);
        }
    };

    info!("starting...");
    if config.debug {
        debug!("debug logging on");
    }

    // Generate random data for testing
    let mut rng = Rng::new();
    let mut test_data = vec![0u8; 1024];
    rng.fill(&mut test_data);

    let (server, _shutdown_tx) = match Server::new(config) {
        Ok(server) => server,
        Err(e) => {
            error!("Failed to initialize server: {}", e);
            return Err(e);
        }
    };

    info!("Starting server...");
    server.run().await?;
    info!("Server stopped");

    Ok(())
}