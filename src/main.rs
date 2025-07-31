use log::{debug, info};
use tokio::signal;

use crate::config::parser::{ read_config_file};
use crate::mioserver::MioServer;
use crate::tokio_server::server::Server;
use crate::tokio_server::server_config::RmbtServerConfig;
use crate::tokio_server::utils::random_buffer;
use std::error::Error as StdError;

pub mod config;
pub mod logger;
pub mod mioserver;
pub mod stream;
pub mod tokio_server;

pub mod client;

#[tokio::main]
async fn main() -> Result<(), Box<dyn StdError + Send + Sync>> {
    let mut args: Vec<String> = std::env::args().collect();

    let config = read_config_file();

    if  args.len() == 1 || args[1] == "-c" {
        args = args.iter().skip(1).map(|s| s.clone()).collect();
        client::client::client_run(args, config).await?;
        return Ok(());
    } else if args[1] == "-s" {
        println!("args: {:?}", args);
        args = args.iter().skip(1).map(|s| s.clone()).collect();

        let mut mio_server = MioServer::new(args, config)?;
        
        // Создаем отдельный поток для обработки сигналов
        let shutdown_signal = mio_server.get_shutdown_signal();
        tokio::spawn(async move {
            signal::ctrl_c().await.expect("Failed to listen for Ctrl+C");
            info!("Ctrl+C received, shutting down server...");
            shutdown_signal.store(true, std::sync::atomic::Ordering::Relaxed);
        });
        
        mio_server.run()?;
        info!("Server stopping...");
        mio_server.shutdown().await?;
        info!("Server stopped");
    } else {

        args = args.iter().skip(1).map(|s| s.clone()).collect();
        let config = match RmbtServerConfig::from_args(args) {
            Ok(config) => config,
            Err(e) => {
                eprintln!("Failed to parse configuration: {}", e);
                return Err(e);
            }
        };       


        random_buffer::init_random_buffer();
        let buf = random_buffer::get_random_buffer();
        debug!("First 10 bytes: {:?}", &buf[..10]);
        debug!("Last 10 bytes: {:?}", &buf[buf.len() - 10..]);

        let (server, _shutdown_tx) = match Server::new(config) {
            Ok(server) => server,
            Err(e) => {
                // error!("Failed to initialize server: {}", e);
                return Err(e);
            }
        };

        info!("Starting server...");
        server.run().await?;
        info!("Server stopped");

        

    }
    Ok(())

}
