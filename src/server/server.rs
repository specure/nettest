use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use std::error::Error;
use log::{info, error};

use crate::utils::token_validator::TokenValidator;
use crate::utils::tls::TlsConfig;
use crate::server::connection_handler::handle_connection;
use crate::server::server_config::ServerConfig;

pub struct Server {
    config: Arc<ServerConfig>,
    token_validator: Arc<TokenValidator>,
    tls_config: Option<Arc<TlsConfig>>,
    data_buffer: Arc<Mutex<Vec<u8>>>,
}

impl Server {
    pub fn new(config: Arc<ServerConfig>) -> Result<Self, Box<dyn Error + Send + Sync>> {
        let tls_config = if !config.cert_file.is_empty() && !config.key_file.is_empty() {
            Some(Arc::new(TlsConfig::new(&config.cert_file, &config.key_file)?))
        } else {
            None
        };

        Ok(Self {
            config,
            token_validator: Arc::new(TokenValidator::new(
                vec!["default_key".to_string()],
                vec!["default_label".to_string()],
                60,  // max_early: 1 minute
                60,  // max_late: 1 minute
            )),
            tls_config,
            data_buffer: Arc::new(Mutex::new(Vec::new())),
        })
    }

    pub async fn run(&self) -> Result<(), Box<dyn Error + Send + Sync>> {
        let listener = TcpListener::bind(self.config.listen_address()).await?;
        info!("Server listening on {}", self.config.listen_address());

        loop {
            match listener.accept().await {
                Ok((socket, addr)) => {
                    info!("New connection from {}", addr);
                    let config = Arc::clone(&self.config);
                    let tls_config = self.tls_config.as_ref().map(Arc::clone);
                    let token_validator = self.token_validator.clone();
                    let data_buffer = self.data_buffer.clone();

                    tokio::spawn(async move {
                        if let Err(e) = handle_connection(
                            socket,
                            token_validator,
                            tls_config,
                            data_buffer,
                        ).await {
                            error!("Error handling connection: {}", e);
                        }
                    });
                }
                Err(e) => {
                    error!("Failed to accept connection: {}", e);
                }
            }
        }
    }
} 