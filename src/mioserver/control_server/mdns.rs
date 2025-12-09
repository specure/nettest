use crate::mioserver::server::ServerConfig;
use anyhow::Result;
use log::{debug, info};
use mdns_sd::{ServiceDaemon, ServiceInfo};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::interval;

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

    // Create ServiceInfo for TCP service
    // IP address will be determined automatically by the library
    let tcp_service_info = ServiceInfo::new(
        service_type,
        &instance_name,
        &format!("{}.local.", instance_name),
        "", // Empty string means automatic IP determination
        tcp_port,
        txt_properties.clone(),
    )?
    .enable_addr_auto(); // Enable automatic IP address determination

    info!("Announcing TCP service: {} on port {}", service_type, tcp_port);
    mdns.register(tcp_service_info)?;

    // If TLS is available, create a separate service for TLS
    if config.cert_path.is_some() && config.key_path.is_some() {
        let tls_port = config.tls_address.port();
        let tls_service_type = "_nettest._tls.local.";
        let tls_instance_name = format!("{}-tls", instance_name);
        let tls_hostname = format!("{}.local.", tls_instance_name);
        
        let mut tls_txt_properties = std::collections::HashMap::new();
        tls_txt_properties.insert("tls_port".to_string(), tls_port.to_string());
        tls_txt_properties.insert("tcp_port".to_string(), tcp_port.to_string());
        
        if let Some(ref version) = config.version {
            tls_txt_properties.insert("version".to_string(), version.clone());
        }
        
        if let Some(ref server_name) = config.server_name {
            tls_txt_properties.insert("server_name".to_string(), server_name.clone());
        }
        
        let tls_service_info = ServiceInfo::new(
            tls_service_type,
            &tls_instance_name,
        &tls_hostname,
        "", // Empty string means automatic IP determination
        tls_port,
            tls_txt_properties,
        )?
        .enable_addr_auto(); // Enable automatic IP address determination

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
