use crate::mioserver::server::ServerConfig;
use anyhow::Result;
use log::{debug, info};
use mdns_sd::{ServiceDaemon, ServiceInfo};
use std::net::{IpAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::interval;

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

/// Starts mDNS service that announces the server in the local network
/// and responds to queries with server configuration via TXT records
pub async fn start_mdns_service(
    config: ServerConfig,
    shutdown_signal: Arc<AtomicBool>,
) -> Result<()> {
    info!("Starting mDNS service...");

    // Create mDNS daemon
    let mdns = ServiceDaemon::new()?;

    // Create mDNS service for TCP
    let tcp_port = config.tcp_address.port();
    let service_type = "_nettest._tcp.local.";
    let instance_name = "nettest";
    
    // Collect TXT records with server configuration
    let mut txt_properties = std::collections::HashMap::new();
    
    // Add basic information
    txt_properties.insert("tcp_port".to_string(), tcp_port.to_string());
    
    if let Some(ref version) = config.version {
        txt_properties.insert("version".to_string(), version.clone());
    }
    
    // Add TLS information if available
    if config.cert_path.is_some() && config.key_path.is_some() {
        let tls_port = config.tls_address.port();
        txt_properties.insert("tls_port".to_string(), tls_port.to_string());
    }
    
    info!("mDNS TXT properties: {:?}", txt_properties);

    // Get local network IP address (non-loopback)
    let local_ip = get_local_network_ip();
    let hostname = local_ip.as_ref()
        .map(|ip| format!("{}.local.", ip.replace('.', "-")))
        .unwrap_or_else(|| format!("{}.local.", instance_name));
    
    info!("Using hostname: {} (IP: {:?})", hostname, local_ip);

    // Create ServiceInfo for TCP service
    // Use local IP if available, otherwise let library determine automatically
    let tcp_service_info = if let Some(ref ip) = local_ip {
        // Explicitly set IP address to avoid loopback
        ServiceInfo::new(
            service_type,
            &instance_name,
            &hostname,
            ip, // Use real network IP address
            tcp_port,
            txt_properties.clone(),
        )?
    } else {
        // Fallback to automatic determination
        ServiceInfo::new(
            service_type,
            &instance_name,
            &hostname,
            "", // Empty string means automatic IP determination
            tcp_port,
            txt_properties.clone(),
        )?
        .enable_addr_auto() // Enable automatic IP address determination
    };

    info!("Announcing TCP service: {} on port {}", service_type, tcp_port);
    mdns.register(tcp_service_info)?;

    // If TLS is available, create a separate service for TLS
    if config.cert_path.is_some() && config.key_path.is_some() {
        let tls_port = config.tls_address.port();
        let tls_service_type = "_nettest._tls.local.";
        let tls_instance_name = format!("{}-tls", instance_name);
        let tls_hostname = local_ip.as_ref()
            .map(|ip| format!("{}-tls.local.", ip.replace('.', "-")))
            .unwrap_or_else(|| format!("{}.local.", tls_instance_name));
        
        let mut tls_txt_properties = std::collections::HashMap::new();
        tls_txt_properties.insert("tls_port".to_string(), tls_port.to_string());
        tls_txt_properties.insert("tcp_port".to_string(), tcp_port.to_string());
        
        if let Some(ref version) = config.version {
            tls_txt_properties.insert("version".to_string(), version.clone());
        }
        
        if let Some(ref server_name) = config.server_name {
            tls_txt_properties.insert("server_name".to_string(), server_name.clone());
        }
        
        let tls_service_info = if let Some(ref ip) = local_ip {
            // Explicitly set IP address to avoid loopback
            ServiceInfo::new(
                tls_service_type,
                &tls_instance_name,
                &tls_hostname,
                ip, // Use real network IP address
                tls_port,
                tls_txt_properties,
            )?
        } else {
            // Fallback to automatic determination
            ServiceInfo::new(
                tls_service_type,
                &tls_instance_name,
                &tls_hostname,
                "", // Empty string means automatic IP determination
                tls_port,
                tls_txt_properties,
            )?
            .enable_addr_auto() // Enable automatic IP address determination
        };

        info!("Announcing TLS service: {} on port {}", tls_service_type, tls_port);
        mdns.register(tls_service_info)?;
    }

    info!("mDNS service started successfully. Service will be discoverable in local network.");
    info!("Clients can query for '_nettest._tcp' or '_nettest._tls' to get server configuration.");

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
    info!("Unregistering mDNS services...");
    mdns.shutdown()?;

    info!("mDNS service stopped");
    Ok(())
}
