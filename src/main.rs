use crate::server::server::Server;
use crate::server::server_config::RmbtServerConfig;
use log::{debug, info};
use std::error::Error as StdError;

pub mod config;
pub mod handlers;
pub mod logger;
pub mod server;
pub mod stream;
pub mod utils;

use crate::utils::random_buffer;

pub mod client;

#[tokio::main]
async fn main() -> Result<(), Box<dyn StdError + Send + Sync>> {
    let mut args: Vec<String> = std::env::args().collect();
    //if first -c or -s skip it
    if args[1] == "-c" {
        args = args.iter().skip(1).map(|s| s.clone()).collect();
        client::async_main(args).await?;
        return Ok(());
    } else if args[1] == "-s" {
        println!("args: {:?}", args);
        args = args.iter().skip(1).map(|s| s.clone()).collect();
        let config = match RmbtServerConfig::from_args(args) {
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
                // error!("Failed to initialize server: {}", e);
                return Err(e);
            }
        };
    
        info!("Starting server...");
        server.run().await?;
        info!("Server stopped");
    } else {
        print_help();
        return Err("Invalid argument".into());
    }

    Ok(())
}


fn print_help() {
    println!("Usage: -c <server_address> : to run client");
    println!("Usage: -s : to run server");

    println!("Usage: nettest -c -h | --help  to print client help");
    println!("Usage: nettest -s -h | --help to print server help");
}