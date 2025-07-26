use log::{debug};
use serde::{Deserialize, Serialize};
use reqwest;
use anyhow::Result;
use std::net::ToSocketAddrs;

#[derive(Debug, Deserialize, Serialize)]
pub struct MeasurementServer {
    pub id: i32,
    pub uuid: Option<String>,
    pub name: String,
    #[serde(rename = "webAddress")]
    pub web_address: String,
    pub provider: Option<Provider>,
    #[serde(rename = "secretKey")]
    pub secret_key: String,
    pub city: String,
    pub email: Option<String>,
    pub company: Option<String>,
    pub expiration: Option<String>,
    #[serde(rename = "ipAddress")]
    pub ip_address: Option<String>,
    pub comment: Option<String>,
    pub countries: Option<Vec<String>>,
    pub location: Location,
    pub distance: f64,
    #[serde(rename = "serverTypeDetails")]
    pub server_type_details: Vec<ServerTypeDetail>,
    pub dedicated: bool,
    #[serde(rename = "ipV4Support")]
    pub ip_v4_support: bool,
    #[serde(rename = "ipV6Support")]
    pub ip_v6_support: bool,
    pub version: Option<String>,
    #[serde(rename = "onNet")]
    pub on_net: bool,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Provider {
    pub id: i32,
    pub name: String,
    pub country: String,
    #[serde(rename = "mnoActive")]
    pub mno_active: bool,
    #[serde(rename = "ispActive")]
    pub isp_active: bool,
    #[serde(rename = "createdDate")]
    pub created_date: String,
    #[serde(rename = "modifiedDate")]
    pub modified_date: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Location {
    pub latitude: f64,
    pub longitude: f64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ServerTypeDetail {
    #[serde(rename = "serverType")]
    pub server_type: String,
    pub port: i32,
    #[serde(rename = "portSsl")]
    pub port_ssl: i32,
    pub encrypted: bool,
}

/// Check if string is an IP address
pub fn is_ip_address(addr: &str) -> bool {
    addr.parse::<std::net::IpAddr>().is_ok()
}

/// Resolve IP address from web address using DNS
pub fn resolve_ip_from_web_address(web_address: &str) -> Result<String> {
    // If it's already an IP address, return it as is
    if is_ip_address(web_address) {
        return Ok(web_address.to_string());
    }
    
    // If it's a hostname, resolve it to IP
    let socket_addrs: Vec<std::net::SocketAddr> = format!("{}:80", web_address)
        .to_socket_addrs()?
        .collect();
    
    if let Some(socket_addr) = socket_addrs.first() {
        Ok(socket_addr.ip().to_string())
    } else {
        Err(anyhow::anyhow!("Failed to resolve IP for {}", web_address))
    }
}

pub async fn fetch_measurement_servers(x_nettest_client: &str, control_server: &str) -> Result<Vec<MeasurementServer>> {
    let client = reqwest::Client::new();
    
    let response = client
        .get(control_server)
        .header("x-nettest-client", x_nettest_client)
        .header("Content-Type", "application/json")
        .body("{}")
        .send()
        .await?;

    let servers: Vec<MeasurementServer> = response.json().await?;
    Ok(servers)
}

pub fn filter_servers_by_version(servers: Vec<MeasurementServer>, min_version: &str) -> Vec<MeasurementServer> {
    servers
        .into_iter()
        .filter(|server| {
            if let Some(version) = &server.version {
                // Parse version string and compare
                if let Ok(server_version) = semver::Version::parse(version) {
                    if let Ok(min_ver) = semver::Version::parse(min_version) {
                        return server_version >= min_ver;
                    }
                }
            }
            false
        })
        .collect()
}

pub fn find_nearest_server(servers: Vec<MeasurementServer>) -> Option<MeasurementServer> {
    servers
        .into_iter()
        .min_by(|a, b| a.distance.partial_cmp(&b.distance).unwrap_or(std::cmp::Ordering::Equal))
}

pub async fn get_best_measurement_server(x_nettest_client: &str, control_server: &str) -> Result<Option<MeasurementServer>> {
    // Fetch all servers
    let servers = fetch_measurement_servers(x_nettest_client, control_server).await?;
    
    // Filter by version > 2.0.0
    let filtered_servers = filter_servers_by_version(servers, "2.0.0");
    
    // Find nearest server
    let mut nearest = find_nearest_server(filtered_servers);
    
    // If we found a server and it has empty IP, resolve it from web_address
    if let Some(ref mut server) = nearest {
        if server.ip_address.is_none() || server.ip_address.as_ref().map(|ip| ip.is_empty()).unwrap_or(true) {
            if let Ok(ip) = resolve_ip_from_web_address(&server.web_address) {
                server.ip_address = Some(ip);
            }
        }
    }

    debug!("Found server: {:?}", nearest);
    
    Ok(nearest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_filtering() {
        let servers = vec![
            MeasurementServer {
                id: 1,
                uuid: None,
                name: "Server 1".to_string(),
                web_address: "test1.com".to_string(),
                provider: None,
                secret_key: "key1".to_string(),
                city: "City1".to_string(),
                email: None,
                company: None,
                expiration: None,
                ip_address: None,
                comment: None,
                countries: None,
                location: Location { latitude: 0.0, longitude: 0.0 },
                distance: 1000.0,
                server_type_details: vec![],
                dedicated: false,
                ip_v4_support: true,
                ip_v6_support: true,
                version: Some("2.1.0".to_string()),
                on_net: false,
            },
            MeasurementServer {
                id: 2,
                uuid: None,
                name: "Server 2".to_string(),
                web_address: "test2.com".to_string(),
                provider: None,
                secret_key: "key2".to_string(),
                city: "City2".to_string(),
                email: None,
                company: None,
                expiration: None,
                ip_address: None,
                comment: None,
                countries: None,
                location: Location { latitude: 0.0, longitude: 0.0 },
                distance: 500.0,
                server_type_details: vec![],
                dedicated: false,
                ip_v4_support: true,
                ip_v6_support: true,
                version: Some("1.9.0".to_string()),
                on_net: false,
            },
        ];

        let filtered = filter_servers_by_version(servers, "2.0.0");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "Server 1");
    }

    #[test]
    fn test_nearest_server() {
        let servers = vec![
            MeasurementServer {
                id: 1,
                uuid: None,
                name: "Far Server".to_string(),
                web_address: "far.com".to_string(),
                provider: None,
                secret_key: "key1".to_string(),
                city: "Far City".to_string(),
                email: None,
                company: None,
                expiration: None,
                ip_address: None,
                comment: None,
                countries: None,
                location: Location { latitude: 0.0, longitude: 0.0 },
                distance: 1000.0,
                server_type_details: vec![],
                dedicated: false,
                ip_v4_support: true,
                ip_v6_support: true,
                version: Some("2.1.0".to_string()),
                on_net: false,
            },
            MeasurementServer {
                id: 2,
                uuid: None,
                name: "Near Server".to_string(),
                web_address: "near.com".to_string(),
                provider: None,
                secret_key: "key2".to_string(),
                city: "Near City".to_string(),
                email: None,
                company: None,
                expiration: None,
                ip_address: None,
                comment: None,
                countries: None,
                location: Location { latitude: 0.0, longitude: 0.0 },
                distance: 500.0,
                server_type_details: vec![],
                dedicated: false,
                ip_v4_support: true,
                ip_v6_support: true,
                version: Some("2.1.0".to_string()),
                on_net: false,
            },
        ];

        let nearest = find_nearest_server(servers);
        assert!(nearest.is_some());
        assert_eq!(nearest.unwrap().name, "Near Server");
    }
} 