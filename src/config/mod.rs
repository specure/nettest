pub mod constants;
pub mod settings;


#[derive(Debug, Clone)]
pub struct Config {
    pub listen_address: String,
    pub tls_listen_address: Option<String>,
    pub cert_path: Option<String>,
    pub key_path: Option<String>,
    pub num_threads: usize,
    pub user: Option<String>,
    pub group: Option<String>,
    pub daemonize: bool,
    pub debug: bool,
    pub use_websocket: bool,
    pub protocol_version: Option<u32>, //TODO None for latest, Some(3) for v0.3 
}

impl Config {
    pub fn set_protocol_version(&mut self, version: &str) -> Result<(), String> {
        if self.protocol_version.is_some() {
            return Err("Only one protocol version is allowed".to_string());
        }

        match version {
            "0.3" => {
                self.protocol_version = Some(3);
                Ok(())
            }
            _ => Err(format!("Unsupported version for backwards compatibility: {}", version))
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            listen_address: "0.0.0.0:8080".to_string(),
            tls_listen_address: None,
            cert_path: None,
            key_path: None,
            num_threads: 4,
            user: None,
            group: None,
            daemonize: false,
            debug: false,
            use_websocket: false,
            protocol_version: None, // Default to latest version
        }
    }
} 