
use log::LevelFilter;

use crate::config::{App, FileConfig};


pub fn read_config_file() -> FileConfig {
    use std::fs;
    use std::path::PathBuf;

    // Определяем путь к конфигурационному файлу в зависимости от ОС
    let config_path = if cfg!(target_os = "macos") {
        PathBuf::from("/usr/local/etc/nettest.conf")
    } else {
        PathBuf::from("/etc/nettest.conf")
    };

    // Проверяем права доступа и читаем файл
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

fn parse_config_content(content: &str) -> FileConfig {
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
                "default_mode" => {
                    if value == "server" {
                        config.app = App::Server;
                    } else {
                        config.app = App::Client;
                    }
                }
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
                    }
                }
                _ => {
                    println!("Warning: Unknown config key: {}", key);
                }
            }
        }
    }

    config
}
