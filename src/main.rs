mod config;
mod handlers;
mod logger;
mod protocol;
mod server;
mod utils;

use std::sync::Arc;
use std::error::Error;
use env_logger;
use clap::{Parser, ValueEnum};
use std::path::PathBuf;
use std::time::Duration;
use log::LevelFilter;

use crate::server::server_config::ServerConfig;
use crate::server::server::Server;

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
    let args = Args::parse();

    // Initialize logger with the specified level
    env_logger::Builder::new()
        .filter_level(args.log_level.into())
        .init();

    // Load configuration
    let mut config = ServerConfig::load(&args.config)?;

    // Override config with command line arguments if provided
    if let Some(label) = args.token_label {
        config.token_label = label;
    }
    if let Some(labels_file) = args.token_labels_file {
        config.token_labels_file = labels_file.to_string_lossy().into_owned();
    }
    if let Some(cert_file) = args.cert_file {
        config.cert_file = cert_file.to_string_lossy().into_owned();
    }
    if let Some(key_file) = args.key_file {
        config.key_file = key_file.to_string_lossy().into_owned();
    }
    if let Some(timeout) = args.timeout {
        config.timeout = Duration::from_secs(timeout);
    }
    if let Some(username) = args.username {
        config.username = username;
    }
    if let Some(password) = args.password {
        config.password = password;
    }
    if let Some(version) = args.version {
        config.version = version;
    }
    if args.debug {
        config.debug = true;
    }
    if args.daemon {
        config.daemon = true;
    }
    if let Some(workers) = args.workers {
        config.num_threads = workers;
    }

    // Create and run server
    let config = Arc::new(config);
    let server = Server::new(config)?;
    server.run().await?;

    Ok(())
}