use crate::server::server::Server;
use crate::server::server_config::ServerConfig;
use std::error::Error;

mod server;
mod protocol;
mod handlers;
mod utils;
mod config;

use clap::{Parser, ValueEnum};
use env_logger;
use log::LevelFilter;
use crate::utils::daemon;

#[derive(ValueEnum, Clone, Debug)]
enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl From<LogLevel> for LevelFilter {
    fn from(level: LogLevel) -> Self {
        match level {
            LogLevel::Error => LevelFilter::Error,
            LogLevel::Warn => LevelFilter::Warn,
            LogLevel::Info => LevelFilter::Info,
            LogLevel::Debug => LevelFilter::Debug,
            LogLevel::Trace => LevelFilter::Trace,
        }
    }
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Number of worker threads
    #[arg(short = 't', long = "threads", default_value_t = 4)]
    num_threads: usize,

    /// Port for plain TCP connections
    #[arg(short = 'l', long = "plain-port", default_value_t = 8080)]
    plain_port: u16,

    /// Port for TLS connections
    #[arg(short = 'L', long = "tls-port", default_value_t = 443)]
    tls_port: u16,

    /// Path to certificate file
    #[arg(short = 'c', long = "cert")]
    cert_path: Option<String>,

    /// Path to private key file
    #[arg(short = 'k', long = "key")]
    key_path: Option<String>,

    /// Run as daemon
    #[arg(short = 'd', long = "daemon")]
    daemon: bool,

    /// Path to config file
    #[arg(short = 'f', long = "config", default_value = "config.toml")]
    config_path: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let args = Args::parse();

    // Parse command line arguments
    let config = ServerConfig::from_args()?;

    if args.daemon {
        // Run as daemon
        daemon::daemonize()?;
    }

    // Create server
    let server = Server::new(config)?;

    // Run server
    server.run().await?;

    Ok(())
}