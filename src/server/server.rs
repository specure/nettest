use std::sync::Arc;
use tokio::net::TcpListener;
use std::error::Error;
use std::net::SocketAddr;
use log::{info, error, debug};
use crate::utils::{token_validator::TokenValidator, user::UserPrivileges};
use crate::server::server_config::RmbtServerConfig;
use tokio::sync::oneshot;
use crate::server::connection_handler::ConnectionHandler;
use tokio::net::TcpStream;
use crate::utils::use_http::{define_stream};
use tokio_rustls::TlsAcceptor;



pub struct Server {
    config: Arc<RmbtServerConfig>,
    tls_acceptor: Option<Arc<TlsAcceptor>>,
    token_validator: Arc<TokenValidator>,
    shutdown_signal: oneshot::Receiver<()>,
}

impl Server {
    pub fn new(
        config: RmbtServerConfig,
    ) -> Result<(Self, oneshot::Sender<()>), Box<dyn Error + Send + Sync>> {
        // Create TLS acceptor if SSL is configured
        let tls_acceptor = if !config.ssl_listen_addresses.is_empty() {
            Some(Arc::new(config.load_identity()?))
        } else {
            None
        };

        let config = Arc::new(config);
        let token_validator = Arc::new(TokenValidator::new(
            config.secret_keys.clone(),
            config.secret_key_labels.clone(),
        ));

        let (shutdown_tx, shutdown_rx) = oneshot::channel();

        Ok((
            Self {
                config,
                tls_acceptor,
                token_validator,
                shutdown_signal: shutdown_rx,
            },
            shutdown_tx,
        ))
    }

    pub async fn run(mut self) -> Result<SocketAddr, Box<dyn Error + Send + Sync>> {
        // Create TCP listeners for plain connections
        let mut plain_listeners = Vec::new();
        for addr in &self.config.listen_addresses {
            let listener = TcpListener::bind(addr).await?;
            info!("Listening on plain TCP {}", addr);
            plain_listeners.push(listener);
        }

        // Create TCP listeners for TLS connections
        let mut tls_listeners = Vec::new();
        for addr in &self.config.ssl_listen_addresses {
            let listener = TcpListener::bind(addr).await?;
            info!("Listening on TLS {}", addr);
            tls_listeners.push(listener);
        }

        // Get the first listening address to return
        let first_addr = plain_listeners
            .first()
            .map(|l| l.local_addr().unwrap())
            .or_else(|| tls_listeners.first().map(|l| l.local_addr().unwrap()))
            .ok_or("No listeners configured")?;

        // Create a vector to hold all listeners
        let mut all_listeners = Vec::new();
        for listener in plain_listeners {
            all_listeners.push((listener, false)); // false means no SSL
        }
        for listener in tls_listeners {
            all_listeners.push((listener, true)); // true means SSL
        }

        if self.config.user_privileges {
            info!("Dropping privileges for user: {}", self.config.user.clone().unwrap());
            let user_privs = UserPrivileges::new(&self.config.user.clone().unwrap())?;
            user_privs.drop_privileges()?;
        }

        // Handle incoming connections
        loop {
            tokio::select! {
                // Handle all listeners
                result = {
                    let mut futures = Vec::new();
                    for (listener, is_ssl) in &all_listeners {
                        let is_ssl = *is_ssl;
                        futures.push(Box::pin(async move {
                            match listener.accept().await {
                                Ok((stream, addr)) => Some((stream, addr, is_ssl)),
                                Err(e) => {
                                    eprintln!("Failed to accept connection: {}", e);
                                    None
                                }
                            }
                        }));
                    }
                    futures::future::select_all(futures)
                } => {
                    if let Some((stream, addr, is_ssl)) = result.0 {
                        let token_validator = self.token_validator.clone();
                        let config = self.config.clone();
                        let tls_acceptor = self.tls_acceptor.clone();
                        tokio::spawn(async move {
                            if let Err(e) = handle_connection(stream, addr, is_ssl, token_validator, config, tls_acceptor).await {
                                error!("Error handling connection: {}", e);
                            }
                        });
                    }
                }
                _ = &mut self.shutdown_signal => {
                    info!("Shutdown signal received");
                    break;
                }
            }
        }

        Ok(first_addr)
    }
}

async fn handle_connection(
    stream: TcpStream,
    addr: SocketAddr,
    is_ssl: bool,
    token_validator: Arc<TokenValidator>,
    config: Arc<RmbtServerConfig>,
    tls_acceptor: Option<Arc<TlsAcceptor>>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    info!("New {} connection from {}", if is_ssl { "TLS" } else { "plain TCP" }, addr);

    let stream = stream;

    let stream = define_stream(stream, tls_acceptor).await?;

    info!("Connection established  {}", stream.to_string());

    let mut handler = ConnectionHandler::new(
        stream,
        config,
        token_validator,
    );

    match handler.handle().await {
        Ok(_) => {
            info!("Connection from {} closed normally", addr);
            Ok(())
        }
        Err(e) => {
            let is_connection_closed = e.to_string().contains("connection closed") ||
                e.to_string().contains("broken pipe") ||
                e.to_string().contains("Connection reset by peer");

            if is_connection_closed {
                debug!("Connection from {} closed by client (error: {})", addr, e);
                Ok(())
            } else {
                debug!("Error handling connection from {}: {}", addr, e);
                Err(e)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpStream;
    use std::net::{TcpListener};
    use log::debug;

    fn find_free_port() -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    }

    #[tokio::test]
    async fn test_server_run() {
        let port = find_free_port();
        debug!("Found free port: {}", port);

        let config = RmbtServerConfig {
            listen_addresses: vec![format!("127.0.0.1:{}", port).parse().unwrap()],
            ssl_listen_addresses: vec![],
            cert_path: Some("test.crt".to_string()),
            key_path: Some("test.key".to_string()),
            num_threads: 1,
            user: None,
            daemon: false,
            debug: true,
            websocket: false,
            version: None,
            secret_keys: vec![],
            secret_key_labels: vec![],
            user_privileges: false,
        };

        let (server, shutdown_tx) = Server::new(config).expect("Failed to create server");
        
        // Run server in a separate task
        let server_handle = tokio::spawn({
            async move {
                server.run().await
            }
        });

        // Try to connect to the server
        match TcpStream::connect(format!("127.0.0.1:{}", port)).await {
            Ok(_) => debug!("Successfully connected to server on port {}", port),
            Err(e) => debug!("Failed to connect to server: {}", e),
        }

        // Send shutdown signal
        let _ = shutdown_tx.send(());

        // Wait for server to shutdown
        match server_handle.await {
            Ok(Ok(addr)) => debug!("Server shutdown successfully on {}", addr),
            Ok(Err(e)) => debug!("Server shutdown with error: {}", e),
            Err(e) => debug!("Server task panicked: {}", e),
        }
    }
}


