use crate::server::server::Server;
use crate::server::server_config::ServerConfig;
use std::error::Error;
use log::{error, info, debug};
use rand::rngs::OsRng;

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

    // Initialize random number generator
    let _rng = OsRng;
    if config.debug {
        debug!("random number generator initialized");
    }

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