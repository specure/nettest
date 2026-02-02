use crate::mioserver::server::ServerConfig;
use anyhow::Result;
use log::{debug, info};
use mdns_sd::{ServiceDaemon, ServiceInfo};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::interval;
use std::net::{IpAddr, UdpSocket};


/// Starts mDNS service that announces the server in the local network
/// and responds to queries with server configuration via TXT records
pub async fn start_mdns_service(
    config: ServerConfig,
    shutdown_signal: Arc<AtomicBool>,
) -> Result<()> {
    info!("Starting mDNS service...");

    // Create mDNS daemon
    let mdns = ServiceDaemon::new()?;

    // Single mDNS service for both TCP and TLS (if available)
    // Both protocols share the same IP address, so we announce one host with both ports in TXT
    let tcp_port = config.tcp_addresses.first().unwrap().port();
    let service_type = "_nettest._tcp.local.";
    let instance_name = "nettest";
    let hostname = format!("{}.local.", instance_name);
    
    // Collect TXT records with server configuration
    let mut txt_properties = std::collections::HashMap::new();
    
    // Always include TCP port (required for local network)
    txt_properties.insert("tcp_port".to_string(), tcp_port.to_string());
    
    // Add TLS port if available (optional, for external access or additional security)
    if config.cert_path.is_some() && config.key_path.is_some() {
        let tls_port = config.tls_addresses.first().unwrap().port();
        txt_properties.insert("tls_port".to_string(), tls_port.to_string());
        info!("TLS available on port {}, will be included in mDNS TXT records", tls_port);
    }
    
    if let Some(ref version) = config.version {
        txt_properties.insert("version".to_string(), version.clone());
    }
    
    info!("mDNS TXT properties: {:?}", txt_properties);

    let ip = get_local_network_ip().unwrap_or_else(|| "".to_string());
    info!("IP address: {}", ip);

    // Create single ServiceInfo with TCP port (TLS port is in TXT records)
    // Clients can use TCP for local network (no TLS needed) or TLS if available
    let service_info = ServiceInfo::new(
        service_type,
        &instance_name,
        &hostname,
        &ip,
        tcp_port, // SRV record points to TCP port
        txt_properties,
    )?;

    info!("Announcing service: {} on {}:{} (TCP port in SRV, TLS port in TXT if available)", 
          service_type, hostname, tcp_port);
    mdns.register(service_info)?;

    info!("mDNS service started successfully. Service will be discoverable in local network.");
    info!("Clients can query for '_nettest._tcp' to get server configuration.");

    // Periodically check shutdown signal
    let mut interval_timer = interval(Duration::from_secs(10));
    
    loop {
        // Check shutdown signal
        if shutdown_signal.load(Ordering::Relaxed) {
            info!("mDNS service received shutdown signal, stopping...");
            break;
        }

        // Wait for the next interval
        interval_timer.tick().await;
        debug!("mDNS service is active and responding to queries");
    }

    // On shutdown, send goodbye packets
    info!("Unregistering mDNS service...");
    mdns.unregister(&format!("{}.local.", instance_name))?;
    mdns.shutdown()?;

    info!("mDNS service stopped");
    Ok(())
}


/// Gets the local non-loopback IP address
/// Returns the first non-loopback, non-link-local IPv4 address found
fn get_local_network_ip() -> Option<String> {
    // Method 1: Try UDP socket connection to external address
    if let Ok(socket) = UdpSocket::bind("0.0.0.0:0") {
        if socket.connect("8.8.8.8:80").is_ok() {
            if let Ok(local_addr) = socket.local_addr() {
                match local_addr.ip() {
                    IpAddr::V4(ipv4) => {
                        // Skip loopback (127.0.0.1) and link-local (169.254.x.x) addresses
                        if !ipv4.is_loopback() && !ipv4.is_link_local() {
                            return Some(ipv4.to_string());
                        }
                    }
                    IpAddr::V6(ipv6) => {
                        // Skip loopback and unspecified addresses
                        if !ipv6.is_loopback() && !ipv6.is_unspecified() {
                            return Some(ipv6.to_string());
                        }
                    }
                }
            }
        }
    }
    
    // Method 2: Try TCP listener as fallback
    if let Ok(listener) = std::net::TcpListener::bind("0.0.0.0:0") {
        if let Ok(addr) = listener.local_addr() {
            match addr.ip() {
                IpAddr::V4(ipv4) if !ipv4.is_loopback() && !ipv4.is_link_local() => {
                    return Some(ipv4.to_string());
                }
                IpAddr::V6(ipv6) if !ipv6.is_loopback() && !ipv6.is_unspecified() => {
                    return Some(ipv6.to_string());
                }
                _ => {}
            }
        }
    }
    
    None
}