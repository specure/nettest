use log::{debug, info};
use tokio::signal;

use crate::config::parser::read_config_file;
use crate::mioserver::MioServer;
use std::error::Error as StdError;

pub mod config;
pub mod logger;
pub mod mioserver;
pub mod stream;
pub mod tokio_server;
pub mod voip;
pub mod udp;

pub mod client;

#[tokio::main]
async fn main() -> Result<(), Box<dyn StdError + Send + Sync>> {
    let mut args: Vec<String> = std::env::args().collect();

    let config_result = read_config_file();
    if config_result.is_err() {
        return Err(config_result.err().unwrap().into());
    }
    let config = config_result.unwrap();
    if args.len() == 1 || args[1] == "-c" {
        args = args.iter().skip(1).map(|s| s.clone()).collect();
        client::client::client_run(args, config).await?;
        return Ok(());
    } else if args[1] == "-s" {
        debug!("args: {:?}", args);
        args = args.iter().skip(1).map(|s| s.clone()).collect();

        let mut mio_server = MioServer::new(args, config)?;

        // Create separate thread for signal handling
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
    } else if args[1] == "-v" || args[1] == "--version" {
        println!("nettest {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    } else {
        let is_help = args[1] == "-h" || args[1] == "--help";
        if !is_help {
            eprintln!("Error: Invalid argument '{}'\n", args[1]);
        }
        println!("nettest - Network speed measurement tool\n");
        println!("USAGE:");
        println!("    nettest              Run client with auto-discovered server");
        println!("    nettest -c [OPTIONS] Run as client");
        println!("    nettest -s [OPTIONS] Run as server\n");
        println!("For detailed help:");
        println!("    nettest -c -h        Show client options");
        println!("    nettest -s -h        Show server options");
        println!("    nettest -v           Print version and exit");
        if !is_help {
            std::process::exit(1);
        }
    }
    Ok(())
}
