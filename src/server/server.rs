use std::sync::Arc;
use tokio::net::TcpListener;
use tokio_native_tls::TlsAcceptor;
use std::error::Error;
use std::net::SocketAddr;
use log::{info, error};
use tokio::signal::unix::{signal, SignalKind};

use crate::server::server_config::ServerConfig;
use tokio::task::JoinHandle;
use tokio::sync::oneshot;
use std::sync::Mutex;
use tokio::time::sleep;
use tokio_native_tls::TlsConnector;
use native_tls::TlsConnector as NativeTlsConnector;

pub struct Server {
    config: Arc<ServerConfig>,
    tls_acceptor: Option<Arc<TlsAcceptor>>,
    data_buffer: Arc<Mutex<Vec<u8>>>,
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
            data_buffer: Arc::new(Mutex::new(Vec::new())),
        })
    }

    pub async fn run(&self) -> Result<SocketAddr, Box<dyn Error + Send + Sync>> {
        // Set up signal handlers
        let mut sigterm = signal(SignalKind::terminate())?;
        let mut sigint = signal(SignalKind::interrupt())?;

        // Create TCP listeners for plain connections
        let mut plain_listeners = Vec::new();
        for addr in &self.config.listen_addresses {
            match TcpListener::bind(addr).await {
                Ok(listener) => {
                    let local_addr = listener.local_addr()?;
                    info!("Listening on plain TCP {}", local_addr);
                    plain_listeners.push((listener, local_addr));
                }
                Err(e) => {
                    error!("Failed to bind plain TCP socket on {}: {}", addr, e);
                    return Err(e.into());
                }
            }
        }

        // Create TCP listeners for TLS connections
        let mut tls_listeners = Vec::new();
        for addr in &self.config.ssl_listen_addresses {
            match TcpListener::bind(addr).await {
                Ok(listener) => {
                    let local_addr = listener.local_addr()?;
                    info!("Listening on TLS {}", local_addr);
                    tls_listeners.push((listener, local_addr));
                }
                Err(e) => {
                    error!("Failed to bind TLS socket on {}: {}", addr, e);
                    return Err(e.into());
                }
            }
        }

        // Store the first address for returning later
        let first_addr = plain_listeners.first()
            .map(|(_, addr)| *addr)
            .or_else(|| tls_listeners.first().map(|(_, addr)| *addr))
            .ok_or_else(|| -> Box<dyn Error + Send + Sync> { 
                "No listening addresses configured".into() 
            })?;

        // Handle plain TCP connections
        for (listener, _) in plain_listeners {
            let config = self.config.clone();
            let data_buffer = self.data_buffer.clone();

            tokio::spawn(async move {
                loop {
                    match listener.accept().await {
                        Ok((socket, addr)) => {
                            info!("New plain TCP connection from {}", addr);
                            let config = config.clone();
                            let data_buffer = data_buffer.clone();

                            tokio::spawn(async move {
                                // if let Err(e) = handle_connection(socket, &config, data_buffer).await {
                                //     error!("Error handling connection: {}", e);
                                // }
                            });
                        }
                        Err(e) => {
                            error!("Failed to accept connection: {}", e);
                        }
                    }
                }
            });
        }

        // Handle TLS connections
        if let Some(tls_acceptor) = &self.tls_acceptor {
            for (listener, _) in tls_listeners {
                let config = self.config.clone();
                let data_buffer = self.data_buffer.clone();
                let tls_acceptor = tls_acceptor.clone();

                tokio::spawn(async move {
                    loop {
                        match listener.accept().await {
                            Ok((socket, addr)) => {
                                info!("New TLS connection from {}", addr);
                                let config = config.clone();
                                let data_buffer = data_buffer.clone();
                                let tls_acceptor = tls_acceptor.clone();

                                tokio::spawn(async move {
                                    match tls_acceptor.accept(socket).await {
                                        Ok(tls_socket) => {
                                            // if let Err(e) = handle_connection(tls_socket, config, data_buffer).await {
                                            //     error!("Error handling TLS connection: {}", e);
                                            // }
                                        }
                                        Err(e) => {
                                            error!("Failed to accept TLS connection: {}", e);
                                        }
                                    }
                                });
                            }
                            Err(e) => {
                                error!("Failed to accept TLS connection: {}", e);
                            }
                        }
                    }
                });
            }
        }

        // Wait for shutdown signal
        tokio::select! {
            _ = sigterm.recv() => {
                info!("Received SIGTERM, shutting down...");
            }
            _ = sigint.recv() => {
                info!("Received SIGINT, shutting down...");
            }
        }

        Ok(first_addr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::net::TcpStream;
    use tokio::time::sleep;
    use std::sync::Arc;
    use std::sync::Mutex;
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
        };

        let server = Server::new(config).expect("Failed to create server");
        
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

        // Send SIGTERM to shutdown
        unsafe {
            libc::kill(libc::getpid(), libc::SIGTERM);
        }

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
        };

        let server = Server::new(config).expect("Failed to create server");
        
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

        // Send SIGTERM to shutdown
        unsafe {
            libc::kill(libc::getpid(), libc::SIGTERM);
        }

        // Wait for server to shutdown
        match server_handle.await {
            Ok(Ok(addr)) => println!("Server shutdown successfully on {}", addr),
            Ok(Err(e)) => println!("Server shutdown with error: {}", e),
            Err(e) => println!("Server task panicked: {}", e),
        }
    }
}