use log::{debug, info};

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

    if args[1] == "-c" {
        args = args.iter().skip(1).map(|s| s.clone()).collect();
        client::client::client_run(args, config).await?;
        return Ok(());
    } else if args[1] == "-s" {
        println!("args: {:?}", args);
        args = args.iter().skip(1).map(|s| s.clone()).collect();

        let mut mio_server = MioServer::new(args, config)?;
        mio_server.run()?;
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
    // } else if args[1] == "-m" {

    //     // env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("debug"))
    //     //     .init();

    //     // let mut tls = false;
    //     // if args[2] == "-tls" {
    //     //     tls = true;
    //     // }
    //     // Запуск MIO TCP сервера на порту 5005
    //     let addr: SocketAddr = parse_listen_address("5005")?;

    //     info!("Starting MIO TCP server on {}", addr);

    //     info!("MIO TCP server stopped");
    // } else {
    //     print_help();
    //     return Err("Invalid argument".into());
    // }

    }
    Ok(())

}

fn print_help() {
    println!("Usage: nettest -c <server_address> : to run nettest measurement client");
    println!("Usage: nettest -s : to run nettest measurement server");
    println!("Usage: nettest -c -h | --help  to print client help");
    println!("Usage: nettest -s -h | --help to print server help");
}
