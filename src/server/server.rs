use std::sync::Arc;
use tokio::net::TcpListener;
use tokio_native_tls::TlsAcceptor;
use std::error::Error;
use std::net::SocketAddr;
use log::{info, error};
use tokio::sync::Mutex;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use crate::utils::token_validator::TokenValidator;
use crate::server::server_config::ServerConfig;
use tokio::sync::oneshot;
use crate::server::connection_handler::ConnectionHandler;
use crate::server::connection_handler::Stream;
use tokio::net::TcpStream;
use crate::utils::use_http::{define_stream};
use tokio_tungstenite;

pub struct Server {
    config: Arc<ServerConfig>,
    tls_acceptor: Option<Arc<TlsAcceptor>>,
    data_buffer: Arc<Mutex<Vec<u8>>>,
    token_validator: Arc<TokenValidator>,
    shutdown_signal: oneshot::Receiver<()>,
}

impl Server {
    pub fn new(
        config: ServerConfig,
    ) -> Result<(Self, oneshot::Sender<()>), Box<dyn Error + Send + Sync>> {
        // Create TLS acceptor if SSL is configured
        let tls_acceptor = if !config.ssl_listen_addresses.is_empty() {
            let identity = config.load_identity()?;
            let acceptor = tokio_native_tls::native_tls::TlsAcceptor::new(identity)?;
            Some(Arc::new(TlsAcceptor::from(acceptor)))
        } else {
            None
        };

        let (tx, rx) = oneshot::channel();
        let token_validator = Arc::new(TokenValidator::new(
            config.secret_keys.clone(),
            config.secret_key_labels.clone(),
        ));

        Ok((Self {
            config: Arc::new(config),
            tls_acceptor,
            data_buffer: Arc::new(Mutex::new(Vec::new())),
            token_validator,
            shutdown_signal: rx,
        }, tx))
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
                        let data_buffer = self.data_buffer.clone();
                        let tls_acceptor = self.tls_acceptor.clone();
                        tokio::spawn(async move {
                            if let Err(e) = handle_connection(stream, addr, is_ssl, token_validator, config, data_buffer, tls_acceptor).await {
                                eprintln!("Error handling connection: {}", e);
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
    config: Arc<ServerConfig>,
    data_buffer: Arc<Mutex<Vec<u8>>>,
    tls_acceptor: Option<Arc<TlsAcceptor>>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    info!("New {} connection from {}", if is_ssl { "TLS" } else { "plain TCP" }, addr);

    let mut stream = stream;

    let stream = define_stream(stream, tls_acceptor).await?;

    info!("Connection established  {}", stream.to_string());

    let mut handler = ConnectionHandler::new(
        stream,
        config,
        token_validator,
        data_buffer,
    );

    if let Err(e) = handler.handle().await {
        error!("Error handling connection from {}: {}", addr, e);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::net::TcpStream;
    use tokio::time::sleep;
    use std::net::{TcpListener, SocketAddr};
    use log::debug;
    use tokio_native_tls::TlsConnector;
    use native_tls::TlsConnector as NativeTlsConnector;

    fn find_free_port() -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    }

    #[tokio::test]
    async fn test_server_run() {
        let port = find_free_port();
        debug!("Found free port: {}", port);

        let config = ServerConfig {
            listen_addresses: vec![format!("127.0.0.1:{}", port).parse().unwrap()],
            ssl_listen_addresses: vec![],
            cert_path: Some("test.crt".to_string()),
            key_path: Some("test.key".to_string()),
            num_threads: 1,
            user: None,
            daemon: false,
            debug: true,
            websocket: false,
            version: Some(1),
            secret_keys: vec![],
            secret_key_labels: vec![],
        };

        let (server, shutdown_tx) = Server::new(config).expect("Failed to create server");
        
        // Run server in a separate task
        let server_handle = tokio::spawn({
            async move {
                server.run().await
            }
        });

        // Give the server time to start
        sleep(Duration::from_millis(500)).await;

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

    #[tokio::test]
    async fn test_server_tls() {
        let port = find_free_port();
        println!("Found free TLS port: {}", port);

        let config = ServerConfig {
            listen_addresses: vec![],
            ssl_listen_addresses: vec![format!("127.0.0.1:{}", port).parse().unwrap()],
            cert_path: Some("measurementservers.crt".to_string()),
            key_path: Some("measurementservers.key".to_string()),
            num_threads: 1,
            user: None,
            daemon: false,
            debug: true,
            websocket: true,
            version: Some(1),
            secret_keys: vec![],
            secret_key_labels: vec![],
        };

        let (server, shutdown_tx) = Server::new(config).expect("Failed to create server");
        
        // Run server in a separate task
        let server_handle = tokio::spawn({
            async move {
                server.run().await
            }
        });

        // Give the server time to start
        sleep(Duration::from_millis(500)).await;

        // Try to connect to the server using TLS
        let tls_connector = NativeTlsConnector::builder()
            .danger_accept_invalid_certs(true) // Для тестов разрешаем невалидные сертификаты
            .build()
            .unwrap();
        let tls_connector = TlsConnector::from(tls_connector);

        match TcpStream::connect(format!("127.0.0.1:{}", port)).await {
            Ok(stream) => {
                println!("Successfully connected to server on port {}", port);
                match tls_connector.connect("localhost", stream).await {
                    Ok(_) => println!("Successfully established TLS connection"),
                    Err(e) => println!("Failed to establish TLS connection: {}", e),
                }
            },
            Err(e) => println!("Failed to connect to server: {}", e),
        }

        // Send shutdown signal
        let _ = shutdown_tx.send(());

        // Wait for server to shutdown
        match server_handle.await {
            Ok(Ok(addr)) => println!("Server shutdown successfully on {}", addr),
            Ok(Err(e)) => println!("Server shutdown with error: {}", e),
            Err(e) => println!("Server task panicked: {}", e),
        }
    }
}