use std::sync::Arc;
use std::time::Duration;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::error::Error;
use std::fs;
use std::path::Path;
use std::str::FromStr;

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct ServerConfig {
    pub listen_addresses: Vec<SocketAddr>,
    pub ssl_listen_addresses: Vec<SocketAddr>,
    pub cert_path: String,
    pub key_path: String,
    pub num_threads: usize,
    pub user: Option<String>,
    pub daemon: bool,
    pub debug: bool,
    pub websocket: bool,
    pub version: Option<String>,
}

impl ServerConfig {
    pub fn from_args() -> Result<Self, Box<dyn Error + Send + Sync>> {
        let mut config = ServerConfig {
            listen_addresses: Vec::new(),
            ssl_listen_addresses: Vec::new(),
            cert_path: String::new(),
            key_path: String::new(),
            num_threads: 200, // Default value from C version
            user: None,
            daemon: false,
            debug: false,
            websocket: false,
            version: None,
        };

        let args: Vec<String> = std::env::args().collect();
        let mut i = 1;
        while i < args.len() {
            match args[i].as_str() {
                "-l" => {
                    i += 1;
                    if i < args.len() {
                        let addr = args[i].parse::<SocketAddr>()?;
                        config.listen_addresses.push(addr);
                    }
                }
                "-L" => {
                    i += 1;
                    if i < args.len() {
                        let addr = args[i].parse::<SocketAddr>()?;
                        config.ssl_listen_addresses.push(addr);
                    }
                }
                "-c" => {
                    i += 1;
                    if i < args.len() {
                        config.cert_path = args[i].clone();
                    }
                }
                "-k" => {
                    i += 1;
                    if i < args.len() {
                        config.key_path = args[i].clone();
                    }
                }
                "-t" => {
                    i += 1;
                    if i < args.len() {
                        config.num_threads = args[i].parse()?;
                    }
                }
                "-u" => {
                    i += 1;
                    if i < args.len() {
                        config.user = Some(args[i].clone());
                    }
                }
                "-d" => {
                    config.daemon = true;
                }
                "-D" => {
                    config.debug = true;
                }
                "-w" => {
                    config.websocket = true;
                }
                "-v" => {
                    i += 1;
                    if i < args.len() {
                        config.version = Some(args[i].clone());
                    }
                }
                "--help" | "-h" => {
                    println!("==== rmbtd ====");
                    println!("command line arguments:\n");
                    println!(" -l/-L  listen on (IP and) port; -L for SSL;");
                    println!("        examples: \"443\",\"1.2.3.4:1234\",\"[2001:1234::567A]:1234\"");
                    println!("        maybe specified multiple times; at least once\n");
                    println!(" -c     path to SSL certificate in PEM format;");
                    println!("        intermediate certificates following server cert in same file if needed");
                    println!("        required\n");
                    println!(" -k     path to SSL key file in PEM format; required\n");
                    println!(" -t     number of worker threads to run for handling connections (default: 200)\n");
                    println!(" -u     drop root privileges and setuid to specified user; must be root\n");
                    println!(" -d     fork into background as daemon (no argument)\n");
                    println!(" -D     enable debug logging (no argument)\n");
                    println!(" -w     use as websocket server (no argument)\n");
                    println!(" -v     behave as version (v) for serving very old clients");
                    println!("        example: \"0.3\"\n");
                    println!("Required are -c,-k and at least one -l/-L option");
                    std::process::exit(0);
                }
                _ => {
                    eprintln!("Unknown option: {}", args[i]);
                    std::process::exit(1);
                }
            }
            i += 1;
        }

        // Validate required options
        if config.cert_path.is_empty() || config.key_path.is_empty() {
            eprintln!("Error: -c and -k options are required");
            std::process::exit(1);
        }

        if config.listen_addresses.is_empty() && config.ssl_listen_addresses.is_empty() {
            eprintln!("Error: at least one -l or -L option is required");
            std::process::exit(1);
        }

        Ok(config)
    }

    pub fn load_identity(&self) -> Result<tokio_native_tls::native_tls::Identity, Box<dyn Error + Send + Sync>> {
        let cert = fs::read(&self.cert_path)?;
        let key = fs::read(&self.key_path)?;
        Ok(tokio_native_tls::native_tls::Identity::from_pkcs8(&cert, &key)?)
    }
}