use std::error::Error;
use crate::server::server::Server;
use crate::server::server_config::ServerConfig;

mod server;
mod protocol;
mod handlers;
mod utils;
mod config;

use env_logger;
use clap::{Parser, ValueEnum};
use std::path::PathBuf;
use std::time::Duration;
use log::LevelFilter;
use tokio::net::TcpListener;
use tokio_native_tls::TlsAcceptor;
use crate::utils::token_validator::TokenValidator;
use log::info;

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
    #[arg(short, long, default_value = "config.toml")]
    config: String,

    #[arg(short, long, value_enum, default_value = "info")]
    log_level: LogLevel,

    #[arg(short = 'l', long = "label", help = "Token label")]
    token_label: Option<String>,

    #[arg(short = 'L', long = "labels", help = "Token labels file")]
    token_labels_file: Option<PathBuf>,

    #[arg(short = 'c', long = "cert", help = "Certificate file")]
    cert_file: Option<PathBuf>,

    #[arg(short = 'k', long = "key", help = "Private key file")]
    key_file: Option<PathBuf>,

    #[arg(short = 't', long = "timeout", help = "Connection timeout in seconds")]
    timeout: Option<u64>,

    #[arg(short = 'u', long = "user", help = "Username")]
    username: Option<String>,

    #[arg(short = 'p', long = "password", help = "Password")]
    password: Option<String>,

    #[arg(short = 'v', long = "version", help = "Protocol version")]
    version: Option<u32>,

    #[arg(short = 'd', long = "debug", help = "Enable debug mode")]
    debug: bool,

    #[arg(short = 'D', long = "daemon", help = "Run as daemon")]
    daemon: bool,

    #[arg(short = 'w', long = "workers", help = "Number of worker threads")]
    workers: Option<usize>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    // Initialize logging
    env_logger::init();

    // Parse command line arguments
    let config = ServerConfig::from_args()?;

    // Create server
    let server = Server::new(config)?;

    // Run server
    server.run().await?;

    Ok(())
}