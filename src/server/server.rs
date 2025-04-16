use std::sync::Arc;
use tokio::net::TcpListener;
use tokio_native_tls::TlsAcceptor;
use std::error::Error;
use log::{info, error};

use crate::server::connection_handler::handle_connection;
use crate::server::server_config::ServerConfig;
use tokio::task::JoinHandle;

pub struct Server {
    config: Arc<ServerConfig>,
    tls_acceptor: Option<Arc<TlsAcceptor>>,
    data_buffer: Arc<tokio::sync::Mutex<Vec<u8>>>,
}

impl Server {
    pub fn new(
        config: ServerConfig,
    ) -> Result<Self, Box<dyn Error + Send + Sync>> {
        // Create TLS acceptor if SSL is configured
        let tls_acceptor = if !config.ssl_listen_addresses.is_empty() {
            let identity = config.load_identity()?;
            let acceptor = tokio_native_tls::native_tls::TlsAcceptor::new(identity)?;
            Some(Arc::new(TlsAcceptor::from(acceptor)))
        } else {
            None
        };

        Ok(Self {
            config: Arc::new(config),
            tls_acceptor,
            data_buffer: Arc::new(tokio::sync::Mutex::new(Vec::new())),
        })
    }

    pub async fn run(&self) -> Result<(), Box<dyn Error + Send + Sync>> {
        let mut handles = Vec::new();

        // Start plain TCP listeners
        for addr in &self.config.listen_addresses {
            let listener = TcpListener::bind(addr).await?;
            info!("Listening on {} (plain)", addr);
            handles.push(self.spawn_listener(listener, false));
        }

        // Start SSL listeners
        for addr in &self.config.ssl_listen_addresses {
            let listener = TcpListener::bind(addr).await?;
            info!("Listening on {} (SSL)", addr);
            handles.push(self.spawn_listener(listener, true));
        }

        // Wait for all listeners to complete
        for handle in handles {
            if let Err(e) = handle.await {
                error!("Listener error: {}", e);
            }
        }

        Ok(())
    }

    fn spawn_listener(&self, listener: TcpListener, use_ssl: bool) -> JoinHandle<()> {
        let config = self.config.clone();
        let tls_acceptor = self.tls_acceptor.clone();
        let data_buffer = self.data_buffer.clone();

        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, addr)) => {
                        info!("New connection from {}", addr);
                        let config = config.clone();
                        let tls_acceptor = tls_acceptor.clone();
                        let data_buffer = data_buffer.clone();

                        tokio::spawn(async move {
                            if let Err(e) = handle_connection(
                                stream,
                                tls_acceptor,
                                data_buffer,
                                use_ssl,
                            ).await {
                                error!("Error handling connection from {}: {}", addr, e);
                            }
                        });
                    }
                    Err(e) => {
                        error!("Error accepting connection: {}", e);
                    }
                }
            }
        })
    }
} 