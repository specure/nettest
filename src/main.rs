use crate::server::server::Server;
use crate::server::server_config::RmbtServerConfig;
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
    let config = match RmbtServerConfig::from_args() {
        Ok(config) => config,
        Err(e) => {
            eprintln!("Failed to parse configuration: {}", e);
            return Err(e);
        }
    };

    debug!("starting...");
    if config.debug {
        debug!("debug logging on");
    }
    debug!("Random buffer initialising!!!!!!!");

    random_buffer::init_random_buffer();
    let buf = random_buffer::get_random_buffer();
    debug!("First 10 bytes: {:?}", &buf[..10]);
    debug!("Last 10 bytes: {:?}", &buf[buf.len() - 10..]);

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
