use crate::mioserver::control_server::auto_registration::{
    deregister_server, register_server, start_ping_job,
};
use crate::mioserver::control_server::mdns::start_mdns_service;
use bytes::BytesMut;
use log::{debug, info, LevelFilter};
use mio::net::{TcpListener, TcpStream};
use mio::Token;
use std::collections::VecDeque;
use std::io::{self, Read};
use std::net::SocketAddr;
use std::net::{IpAddr, Ipv4Addr};
#[cfg(unix)]
use libc;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::thread;
use std::time::Instant;

#[derive(Debug)]
pub enum ConnectionType {
    Tcp(TcpStream, SocketAddr),
    Tls(TcpStream, SocketAddr), // Same TcpStream but with TLS flag
}

use crate::config::FileConfig;
use crate::mioserver::worker::WorkerThread;
use crate::mioserver::ServerTestPhase;
use crate::stream::stream::Stream;

pub struct MioServer {
    tcp_listeners: Vec<TcpListener>,
    tls_listeners: Vec<TcpListener>,
    static_files_listener: Option<TcpListener>,
    _worker_threads: Vec<WorkerThread>,
    global_queue: Arc<Mutex<VecDeque<(ConnectionType, Instant)>>>, // Global queue with timestamps
    server_config: ServerConfig,
    shutdown_signal: Arc<AtomicBool>,
}

pub struct TestState {
    pub token: Token,
    pub connection_start: Instant,
    pub stream: Stream,
    pub measurement_state: ServerTestPhase,
    pub read_buffer: [u8; 1024 * 8],
    pub write_buffer: [u8; 1024 * 8],
    pub read_bytes: BytesMut,
    pub read_pos: usize,
    pub total_bytes_received: u64,
    pub total_bytes_sent: u64,
    pub write_pos: usize,
    pub num_chunks: usize,
    pub chunk_size: usize,
    pub processed_chunks: usize,
    pub clock: Option<Instant>,
    pub sent_time_ns: Option<u128>,
    pub received_time_ns: Option<u128>,
    pub duration: u64,
    pub put_duration: Option<u128>,
    pub chunk_buffer: Vec<u8>,
    pub loop_iteration_count: u32, // Counter for loop iterations
    pub chunk: Option<BytesMut>,
    pub terminal_chunk: Option<BytesMut>,
    pub bytes_received: VecDeque<(u64, u64)>,
    pub client_addr: Option<SocketAddr>,
    pub sig_key: Option<String>,
}

#[derive(Clone)]
pub struct ServerConfig {
    pub tcp_addresses: Vec<SocketAddr>,
    pub tls_addresses: Vec<SocketAddr>,
    pub cert_path: Option<String>,
    pub key_path: Option<String>,
    pub num_workers: Option<usize>,
    pub user: Option<String>,
    pub daemon: bool,
    pub version: Option<String>,
    pub secret_key: String,
    pub log_level: Option<LevelFilter>,
    pub server_registration: bool,
    pub control_server: String,
    pub hostname: Option<String>,
    pub x_nettest_client: String,
    pub registration_token: Option<String>,
    pub server_name: Option<String>,
    pub enable_mdns: bool,
}

impl MioServer {
    pub fn new(args: Vec<String>, config: FileConfig) -> io::Result<Self> {
        let server_config = crate::mioserver::parser::parse_args(args, config.clone())
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        let mut tcp_listeners = Vec::new();
        let mut tls_listeners = Vec::new();

        for addr in &server_config.tcp_addresses {
            match if addr.is_ipv6() {
                Self::bind_ipv6_with_v6only(*addr)
            } else {
                TcpListener::bind(*addr)
            } {
                Ok(listener) => {
                    info!("TCP Server listening on {}", addr);
                    tcp_listeners.push(listener);
                }
                Err(e) => {
                    info!("Failed to bind TCP V4 listener: {}. On linux it can be because of IPv4-mapped addresses", e);
                }
            };
        }

        for addr in &server_config.tls_addresses {
            if server_config.cert_path.is_some() && server_config.key_path.is_some() {
                match if addr.is_ipv6() {
                    Self::bind_ipv6_with_v6only(*addr)
                } else {
                    TcpListener::bind(*addr)
                } {
                    Ok(listener) => {
                        info!("TLS Server listening on {}", addr);
                        tls_listeners.push(listener);
                    }
                    Err(e) => {
                        info!("Failed to bind TLS listener: {}", e);
                    }
                }
            } else {
                println!("Key and certificate files are not provided, skipping TLS listener");
            }
        }

        let static_files_listener = if server_config.enable_mdns {
            info!("Static files server listening on {}", 5006);
            match TcpListener::bind(SocketAddr::from((IpAddr::V4(Ipv4Addr::UNSPECIFIED), 5006))) {
                Ok(listener) => Some(listener),
                Err(e) => {
                    debug!("Failed to bind static files listener: {}", e);
                    None
                }
            }
        } else {
            None
        };

        let logical = server_config.num_workers.unwrap_or(30);

        let worker_connection_counts = Arc::new(Mutex::new(vec![0; logical]));
        let global_queue = Arc::new(Mutex::new(VecDeque::new()));

        let mut worker_threads = Vec::new();

        for i in 0..logical {
            let worker = WorkerThread::new(
                i,
                worker_connection_counts.clone(),
                global_queue.clone(),
                server_config.clone(),
            )?;
            worker_threads.push(worker);
        }

        Ok(Self {
            tcp_listeners,
            tls_listeners,
            static_files_listener,
            _worker_threads: worker_threads,
            global_queue,
            server_config,
            shutdown_signal: Arc::new(AtomicBool::new(false)),
        })
    }

    pub fn run(&mut self) -> io::Result<()> {
        info!(
            "server_config.server_registration: {:?}",
            self.server_config.server_registration
        );
        if self.server_config.server_registration {
            info!("Registering server with control server...");
            let config_clone = self.server_config.clone();
            info!("Registering server with control server...");
            let shutdown_signal = self.shutdown_signal.clone();
            tokio::spawn(async move {
                match register_server(&config_clone).await {
                    Ok(_) => {
                        info!("Server registration successful, starting ping job...");
                        start_ping_job(config_clone, shutdown_signal).await;
                    }
                    Err(e) => {
                        info!("Server registration failed: {}", e);
                    }
                }
            });
        }

        if self.server_config.enable_mdns {
            info!("Starting mDNS service for local network discovery...");
            let mdns_config = self.server_config.clone();
            let mdns_shutdown = self.shutdown_signal.clone();
            tokio::spawn(async move {
                if let Err(e) = start_mdns_service(mdns_config, mdns_shutdown).await {
                    log::warn!("mDNS service error: {}", e);
                }
            });
        } else {
            debug!("mDNS service disabled (use -mdns flag to enable)");
        }

        loop {
            // Check shutdown signal
            if self.shutdown_signal.load(Ordering::Relaxed) {
                info!("Shutdown signal received, stopping server...");
                break;
            }

            // Accept TCP connections
            let mut tcp_connections = Vec::new();
            for listener in &self.tcp_listeners {
                match listener.accept() {
                    Ok((stream, addr)) => {
                        if let Err(e) = stream.set_nodelay(true) {
                            debug!("Failed to set TCP_NODELAY: {}", e);
                        }
                        tcp_connections.push((stream, addr));
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                        // Continue - no connections available
                    }
                    Err(e) => {
                        debug!("Error accepting TCP connection: {}", e);
                        return Err(e);
                    }
                }
            }
            // Handle connections after releasing the borrow
            for (stream, addr) in tcp_connections {
                self.handle_connection(stream, false, addr)?;
            }

            // Accept TLS connections if listener exists
            let mut tls_connections = Vec::new();
            for listener in &self.tls_listeners {
                match listener.accept() {
                    Ok((stream, addr)) => {
                        if let Err(e) = stream.set_nodelay(true) {
                            debug!("Failed to set TCP_NODELAY: {}", e);
                        }
                        tls_connections.push((stream, addr));
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                        // Continue - no connections available
                    }
                    Err(e) => {
                        debug!("Error accepting TLS connection: {}", e);
                        return Err(e);
                    }
                }
            }
            // Handle connections after releasing the borrow
            for (stream, addr) in tls_connections {
                self.handle_connection(stream, true, addr)?;
            }

            // Accept static files connections if listener exists
            if let Some(ref mut static_listener) = self.static_files_listener {
                match static_listener.accept() {
                    Ok((_mio_stream, addr)) => {
                        info!("Accepting static files connections...");

                        // Convert to std::net::TcpStream for synchronous handling
                        #[cfg(unix)]
                        let std_stream = {
                            use std::os::unix::io::{FromRawFd, AsRawFd};
                            let fd = _mio_stream.as_raw_fd();
                            unsafe { std::net::TcpStream::from_raw_fd(fd) }
                        };
                        
                        #[cfg(windows)]
                        let std_stream = {
                            use std::os::windows::io::{FromRawSocket, AsRawSocket};
                            let socket = _mio_stream.as_raw_socket();
                            unsafe { std::net::TcpStream::from_raw_socket(socket) }
                        };
                        
                        std::mem::forget(_mio_stream); // Don't drop mio stream, we use std_stream

                        // Handle in separate thread to not block main loop
                        let config_clone = self.server_config.clone();
                        std::thread::spawn(move || {
                            if let Err(e) = Self::handle_static_file_connection_sync(std_stream, addr, config_clone.enable_mdns) {
                                debug!("Error handling static file connection: {}", e);
                            }
                        });
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                        // Continue
                    }
                    Err(e) => {
                        debug!("Error accepting static files connection: {}", e);
                        return Err(e);
                    }
                }
            }

            // Check global queue for stale connections
            self.check_global_queue()?;

            thread::sleep(std::time::Duration::from_millis(10));
        }

        Ok(())
    }

    pub async fn shutdown(&mut self) -> io::Result<()> {
        info!("Starting graceful shutdown...");

        if self.server_config.server_registration {
            info!("Deregistering server from control server...");
            let _ = deregister_server(&self.server_config).await;
        }

        info!("Server shutdown complete");
        Ok(())
    }

    pub fn request_shutdown(&self) {
        self.shutdown_signal.store(true, Ordering::Relaxed);
        info!("Shutdown requested");
    }

    pub fn get_shutdown_signal(&self) -> Arc<AtomicBool> {
        self.shutdown_signal.clone()
    }

    fn check_global_queue(&mut self) -> io::Result<()> {
        Ok(())
    }

    fn handle_connection(
        &mut self,
        stream: TcpStream,
        is_tls: bool,
        client_addr: SocketAddr,
    ) -> io::Result<()> {
        let connection = if is_tls {
            ConnectionType::Tls(stream, client_addr)
        } else {
            ConnectionType::Tcp(stream, client_addr)
        };

        // Add connection to global queue
        let mut global_queue = self.global_queue.lock().unwrap();
        global_queue.push_back((connection, Instant::now()));

        info!(
            "{} connection added to global queue (queue size: {})",
            if is_tls { "TLS" } else { "TCP" },
            global_queue.len()
        );

        Ok(())
    }

    fn handle_static_file_connection_sync(
        mut stream: std::net::TcpStream,
        client_addr: SocketAddr,
        mdns_enabled: bool,
    ) -> io::Result<()> {
        use crate::mioserver::handlers::static_files::{
            is_static_file_request, parse_http_path, serve_static_file,
        };
        use crate::stream::stream::Stream;
        use mio::net::TcpStream as MioTcpStream;

        if !mdns_enabled {
            return Ok(()); // Static files only when mdns is enabled
        }

        info!("Static file connection from {}", client_addr);

        stream.set_read_timeout(Some(std::time::Duration::from_secs(5)))?;
        stream.set_write_timeout(Some(std::time::Duration::from_secs(5)))?;

        // Read HTTP request
        let mut buffer = vec![0; 4096];
        let mut request_data = Vec::new();

        loop {
            match stream.read(&mut buffer) {
                Ok(0) => break, // EOF
                Ok(n) => {
                    request_data.extend_from_slice(&buffer[..n]);
                    // Check if we have complete HTTP request
                    if request_data.len() >= 4
                        && (request_data[request_data.len() - 4..] == [b'\r', b'\n', b'\r', b'\n']
                            || String::from_utf8_lossy(&request_data).contains("\r\n\r\n"))
                    {
                        break;
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    continue;
                }
                Err(e) => {
                    debug!("Error reading static file request: {}", e);
                    return Ok(()); // Just close connection
                }
            }
        }

        let request = String::from_utf8_lossy(&request_data);
        debug!("Static file request: {}", request);

        if !is_static_file_request(&request) {
            debug!("Not a static file request, closing connection");
            return Ok(());
        }

        if let Some(path) = parse_http_path(&request) {
            // Convert std::net::TcpStream to mio::net::TcpStream for Stream wrapper
            #[cfg(unix)]
            let mio_stream = {
                use std::os::unix::io::{AsRawFd, FromRawFd};
                let fd = stream.as_raw_fd();
                unsafe { MioTcpStream::from_raw_fd(fd) }
            };

            #[cfg(windows)]
            let mio_stream = {
                use std::os::windows::io::{AsRawSocket, FromRawSocket};
                let socket = stream.as_raw_socket();
                unsafe { MioTcpStream::from_raw_socket(socket) }
            };

            std::mem::forget(stream); // Don't drop std_stream, we use mio_stream

            let mut stream_wrapper = Stream::Tcp(mio_stream);
            match serve_static_file(&path, &mut stream_wrapper) {
                Ok(_) => {
                    info!("Served static file: {} to {}", path, client_addr);
                }
                Err(e) => {
                    debug!("Failed to serve static file {}: {}", path, e);
                    // 404 already sent by serve_static_file
                }
            }
        }

        Ok(())
    }

    #[cfg(unix)]
    fn bind_ipv6_with_v6only(addr: SocketAddr) -> io::Result<TcpListener> {
        use std::os::unix::io::FromRawFd;
        
        // Extract IPv6 address and port
        let addr_v6 = match addr {
            SocketAddr::V6(addr) => addr,
            _ => return Err(io::Error::new(io::ErrorKind::InvalidInput, "Not an IPv6 address")),
        };
        
        // Create IPv6 socket
        let fd = unsafe {
            libc::socket(libc::AF_INET6, libc::SOCK_STREAM, 0)
        };
        
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        
        // Set IPV6_V6ONLY before bind
        let v6only: libc::c_int = 1;
        let result = unsafe {
            libc::setsockopt(
                fd,
                libc::IPPROTO_IPV6,
                libc::IPV6_V6ONLY,
                &v6only as *const _ as *const libc::c_void,
                std::mem::size_of::<libc::c_int>() as libc::socklen_t,
            )
        };
        
        if result != 0 {
            unsafe { libc::close(fd); }
            return Err(io::Error::last_os_error());
        }
        
        // Set non-blocking mode
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if flags < 0 {
            unsafe { libc::close(fd); }
            return Err(io::Error::last_os_error());
        }
        let result = unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) };
        if result != 0 {
            unsafe { libc::close(fd); }
            return Err(io::Error::last_os_error());
        }
        
        // Prepare sockaddr_in6 structure
        // Note: macOS (BSD) requires sin6_len field, Linux doesn't have it
        let ip = addr_v6.ip().octets();
        let port = addr_v6.port();
        let flowinfo = addr_v6.flowinfo();
        let scope_id = addr_v6.scope_id();
        
        let sockaddr = {
            #[cfg(target_os = "macos")]
            {
                libc::sockaddr_in6 {
                    sin6_len: std::mem::size_of::<libc::sockaddr_in6>() as u8,
                    sin6_family: libc::AF_INET6 as u8,
                    sin6_port: port.to_be(),
                    sin6_flowinfo: flowinfo,
                    sin6_addr: libc::in6_addr { s6_addr: ip },
                    sin6_scope_id: scope_id,
                }
            }
            #[cfg(not(target_os = "macos"))]
            {
                libc::sockaddr_in6 {
                    sin6_family: libc::AF_INET6 as u16,
                    sin6_port: port.to_be(),
                    sin6_flowinfo: flowinfo,
                    sin6_addr: libc::in6_addr { s6_addr: ip },
                    sin6_scope_id: scope_id,
                }
            }
        };
        
        // Bind socket
        let bind_result = unsafe {
            libc::bind(
                fd,
                &sockaddr as *const _ as *const libc::sockaddr,
                std::mem::size_of::<libc::sockaddr_in6>() as libc::socklen_t,
            )
        };
        
        if bind_result != 0 {
            unsafe { libc::close(fd); }
            return Err(io::Error::last_os_error());
        }
        
        // Listen on socket
        let listen_result = unsafe {
            libc::listen(fd, 128) // backlog = 128
        };
        
        if listen_result != 0 {
            unsafe { libc::close(fd); }
            return Err(io::Error::last_os_error());
        }
        
        // Convert to std::net::TcpListener
        let std_listener = unsafe {
            std::net::TcpListener::from_raw_fd(fd)
        };
        
        // Convert to mio::TcpListener
        let mio_listener = TcpListener::from_std(std_listener);
        
        debug!("Successfully set IPV6_V6ONLY and bound IPv6 listener on {}", addr);
        Ok(mio_listener)
    }
    
    #[cfg(not(unix))]
    fn bind_ipv6_with_v6only(addr: SocketAddr) -> io::Result<TcpListener> {
        // On Windows, IPV6_V6ONLY is set by default, so just bind normally
        TcpListener::bind(addr)
    }

}

impl Drop for MioServer {
    fn drop(&mut self) {
        debug!("MIO TCP Server shutting down");
    }
}
