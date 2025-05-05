use crate::server::server::Server;
use crate::server::server_config::ServerConfig;
use log::{debug, error, info};
use std::error::Error;

pub mod config;
pub mod handlers;
pub mod logger;
pub mod server;
pub mod stream;
pub mod utils;
use crate::utils::random_buffer;

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
    log::info!("Random buffer initialising!!!!!!!");

    let buf = &random_buffer::RANDOM_BUFFER;
    info!("First 10 bytes: {:?}", &buf[..10]);
    info!("Last 10 bytes: {:?}", &buf[buf.len() - 10..]);
    log::info!("Random buffer initialized!");

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
