use crate::mioserver::server::ServerConfig;
use anyhow::Result;
use log::info;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::time::{interval, Duration};

#[derive(Debug, Serialize)]
struct AutoMeasurementServerRegistrationRequest {
    token: Option<String>,
    #[serde(rename = "tlsPort")]
    tls_port: Option<i32>,
    #[serde(rename = "tcpPort")]
    tcp_port: i32,
    version: Option<String>,
    hostname: Option<String>,
}

#[derive(Debug, Serialize)]
struct PingRequest {
    message: String,
}

pub async fn register_server(config: &ServerConfig) -> Result<()> {
    let control_server_url = config.control_server.clone();

    //if hostname isSome and keyPath is some and certPath is some then tlsPort is Some(tls_address.port()) else tlsPort is None
    let tls_port = if config.hostname.is_some() && config.key_path.is_some() && config.cert_path.is_some() {
        Some(config.tls_address.port() as i32)
    } else {
        None
    };
    
    let request = AutoMeasurementServerRegistrationRequest {
        token: config.secret_key.clone(),
        tls_port,
        tcp_port: config.tcp_address.port() as i32,
        version: config.version.clone(),
        hostname: config.hostname.clone(),
    };
    info!("Registering server with control server json: {:?}", serde_json::to_string(&request).unwrap());
    
    let client = reqwest::Client::new();
    let url = format!("{}/measurementServer/auto-register", control_server_url);
    
    let response = client
        .post(&url)
        .header("x-nettest-client", config.x_nettest_client.clone())
        .header("Content-Type", "application/json")
        .json(&request)
        .send()
        .await?;
    
    let status = response.status();
    if !status.is_success() {
        let error_text = response.text().await?;
        info!("Failed to register server: {} - {}", status, error_text);
        return Err(anyhow::anyhow!(
            "Failed to register server: {} - {}",
            status,
            error_text
        ));
    }
    
    info!("Successfully registered server with control server");
    Ok(())
}

pub async fn ping_server(config: &ServerConfig) -> Result<()> {
    let control_server_url = config.control_server.clone();
    
    let request = PingRequest {
        message: "PING".to_string(),
    };
    
    let client = reqwest::Client::new();
    let url = format!("{}/measurementServer/ping", control_server_url);
    
    let response = client
        .post(&url)
        .header("x-nettest-client", config.x_nettest_client.clone())
        .header("Content-Type", "application/json")
        .json(&request)
        .send()
        .await?;
    
    let status = response.status();
    if !status.is_success() {
        let error_text = response.text().await?;
        info!("Failed to ping server: {} - {}", status, error_text);
        return Err(anyhow::anyhow!(
            "Failed to ping server: {} - {}",
            status,
            error_text
        ));
    }
    
    info!("Successfully pinged control server");
    Ok(())
}

pub async fn deregister_server(config: &ServerConfig) -> Result<()> {
    let control_server_url = config.control_server.clone();
    
    info!("Deregistering server with control server");
    
    let client = reqwest::Client::new();
    let url = format!("{}/measurementServer/auto-deregister", control_server_url);
    
    let response = client
        .delete(&url)
        .header("x-nettest-client", config.x_nettest_client.clone())
        .header("Content-Type", "application/json")
        .send()
        .await?;
    
    let status = response.status();
    if !status.is_success() {
        let error_text = response.text().await?;
        return Err(anyhow::anyhow!(
            "Failed to deregister server: {} - {}",
            status,
            error_text
        ));
    }
    
    info!("Successfully deregistered server with control server");
    Ok(())
}

pub async fn start_ping_job(config: ServerConfig, shutdown_signal: Arc<AtomicBool>) {
    let mut interval_timer = interval(Duration::from_secs(10));
    
    info!("Starting ping job - will ping control server every 10 seconds");
    
    loop {
        info!("Ping job loop");
        // Проверяем сигнал завершения
        if shutdown_signal.load(Ordering::Relaxed) {
            info!("Ping job received shutdown signal, stopping...");
            break;
        }
        
        // Ждем следующего интервала
        interval_timer.tick().await;
        
        // Отправляем ping
        if let Err(e) = ping_server(&config).await {
            info!("Ping job error: {}", e);
        }
    }
    
    info!("Ping job stopped");
}