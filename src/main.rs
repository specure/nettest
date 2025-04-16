use crate::server::server::Server;
use crate::server::server_config::ServerConfig;
use std::error::Error;
use log::{error, info};

mod server;
mod protocol;
mod handlers;
mod utils;
mod config;
mod logger;

use clap::Parser;
use env_logger;
use log::LevelFilter;
use crate::utils::daemon;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let config = match ServerConfig::from_args() {
        Ok(config) => config,
        Err(e) => {
            eprintln!("Failed to parse configuration: {}", e);
            return Err(e);
        }
    };


    let server = match Server::new(config) {
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