pub mod config;
pub mod handlers;
pub mod logger;
pub mod stream;
pub mod utils;

use log::{debug, info};
use rustls::pki_types::{IpAddr, ServerName};

use std::{error::Error, net::SocketAddr, sync::Arc};

use mio::{net::TcpStream, Interest, Poll, Token};

use crate::stream::Stream;
use std::env;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let args: Vec<String> = env::args().collect();
    let use_tls = args.iter().any(|arg| arg == "-tls");
    let use_websocket = args.iter().any(|arg| arg == "-ws");
    let perf_test = args.iter().any(|arg| arg == "-perf");

    let log = args.iter().any(|arg| arg == "-log");
    if log {
        env_logger::init();
    }

    
    let default_server = String::from("127.0.0.1");

    

    


    let addr = format!("{}:5005", default_server).parse::<SocketAddr>()?;


    handlers::handle_greeting(&mut stream, use_websocket).await?;

    // handlers::handle_get_time(&mut stream).await?;

    handlers::handle_put_no_result(&mut stream).await?;

    println!("Hello, world!");
    Ok(())
}
