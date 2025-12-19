
use log::LevelFilter;

use crate::config::{FileConfig};


pub fn read_config_file() -> Result<FileConfig, anyhow::Error> {
    use std::fs;
    use std::path::PathBuf;
    use std::env;

    // Determine config file path based on OS
    let config_path = if cfg!(target_os = "macos") {
        // On macOS use ~/.config/ as main directory
        let home_dir = env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        PathBuf::from(format!("{}/.config/nettest.conf", home_dir))
    } else {
        PathBuf::from("/etc/nettest.conf")
    };

    // Check permissions and read file
    let config_content = if config_path.exists() {
        match fs::read_to_string(&config_path) {
            Ok(content) => {
                println!("Reading config from: {:?}", config_path);
                content
            }
            Err(e) => {
                println!(
                    "Warning: Could not read config file {:?}: {}",
                    config_path, e
                );
                String::new()
            }
        }
    } else {
        if let Some(parent) = config_path.parent() {
            if !parent.exists() {
                if let Err(e) = fs::create_dir_all(parent) {
                    println!("Warning: Could not create config directory: {}", e);
                }
            }
        }
        let default_config = include_str!("../../nettest.conf");

        if let Err(e) = fs::write(&config_path, default_config) {
            println!(
                "Warning: Could not create config file {:?}: {}",
                config_path, e
            );
        } else {
            println!("Created default config file at: {:?}", config_path);
        }

        default_config.to_string()
    };

    parse_config_content(&config_content)
}

fn parse_config_content(content: &str) -> Result<FileConfig, anyhow::Error> {
    let mut config = FileConfig::default();

    for line in content.lines() {
        let line = line.trim();

        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim();
            let value = value.trim().trim_matches('"');

            match key {
                "server_tcp_port" => {
                    if let Ok(port) = value.parse::<u16>() {
                        config.server_tcp_port = port.to_string();
                    } else {
                        config.server_tcp_port = "5005".to_string();
                    }
                }
                "server_tls_port" => {
                    if let Ok(port) = value.parse::<u16>() {
                        config.server_tls_port = Some(port.to_string());
                    } else {
                        config.server_tls_port = Some("443".to_string());
                    }
                }
                "cert_path" => config.cert_path = Some(value.to_string()),
                "key_path" => config.key_path = Some(value.to_string()),
                "server_workers" => {
                    if let Ok(threads) = value.parse::<usize>() {
                        config.server_workers = Some(threads);
                    }
                }
                "user" => config.user = Some(value.to_string()),
                "daemonize" => config.daemonize = value.parse().unwrap_or(false),
                "use_websocket" => config.use_websocket = value.parse().unwrap_or(false),
                "protocol_version" => {
                    if let Ok(version) = value.parse::<u32>() {
                        config.protocol_version = Some(version);
                    }
                }
                // Client-specific settings
                "client_use_tls" => {
                    if value == "true" {
                        config.client_use_tls = true;
                    } else {
                        config.client_use_tls = false;
                    }
                }
                "client_use_websocket" => {
                    if value == "true" {
                        config.client_use_websocket = true;
                    } else {
                        config.client_use_websocket = false;
                    }
                }
                // Logging settings
                "logger" => {
                    if value == "info" {
                        config.logger = LevelFilter::Info;
                    } else if value == "debug" {
                        config.logger = LevelFilter::Debug;
                    } else if value == "trace" {
                        config.logger = LevelFilter::Trace;
                    } else {
                        println!("Unknown logger level: {}, using default", value);
                        return Err(anyhow::anyhow!("Unknown logger level: {}", value));
                    }
                }
                "server_registration" => {
                    if value == "true" {
                        config.server_registration = true;
                    } else {
                        config.server_registration = false;
                    }
                }
                "server_name" => config.server_name = Some(value.to_string()),
                "registration_token" => config.registration_token = Some(value.to_string()),
                "hostname" => config.hostname = Some(value.to_string()),
                "x_nettest_client" => config.x_nettest_client = value.to_string(),
                "control_server" => config.control_server = value.to_string(),
                "client_uuid" => {
                    // Remove quotes if present
                    let clean_value = value.trim_matches('"').trim();
                    if !clean_value.is_empty() {
                        config.client_uuid = Some(clean_value.to_string());
                    }
                }
                "signed_result" => {
                    config.signed_result = value.parse().unwrap_or(false);
                }
                "enable_mdns" => {
                    if value == "true" {
                        config.enable_mdns = true;
                    } else {
                        config.enable_mdns = false;
                    }
                }
                "max_chunk_size" => {
                    if let Ok(size) = value.parse::<u32>() {
                        config.max_chunk_size = Some(size);
                    } else {
                        println!("Warning: Invalid max_chunk_size value: {}, using default", value);
                    }
                }
                _ => {
                    //return error
                    println!("Warning: Unknown config key: {}", key);
                    return Err(anyhow::anyhow!("Unknown config key: {}", key));
                }
            }
        }
    }

    Ok(config)
}
