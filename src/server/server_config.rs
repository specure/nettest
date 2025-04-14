use std::sync::Arc;
use std::time::Duration;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

#[derive(Debug, Serialize, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub version: u32,
    pub timeout: Duration,
    pub num_threads: usize,
    pub token_label: String,
    pub token_labels_file: String,
    pub cert_file: String,
    pub key_file: String,
    pub username: String,
    pub password: String,
    pub debug: bool,
    pub daemon: bool,
    pub log_level: String,
}

impl ServerConfig {
    pub fn new() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        Ok(Self {
            host: "0.0.0.0".to_string(),
            port: 8080,
            version: 2,
            timeout: Duration::from_secs(30),
            num_threads: 200,
            token_label: "test_label".to_string(),
            token_labels_file: "labels.txt".to_string(),
            cert_file: "cert.pem".to_string(),
            key_file: "key.pem".to_string(),
            username: "admin".to_string(),
            password: "password".to_string(),
            debug: false,
            daemon: false,
            log_level: "info".to_string(),
        })
    }

    pub fn load(path: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let config = if std::path::Path::new(path).exists() {
            let contents = std::fs::read_to_string(path)?;
            toml::from_str(&contents)?
        } else {
            Self::new()?
        };
        Ok(config)
    }

    pub fn listen_address(&self) -> SocketAddr {
        format!("{}:{}", self.host, self.port).parse().unwrap()
    }
} 