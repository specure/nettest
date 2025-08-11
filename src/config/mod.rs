use log::LevelFilter;

pub mod constants;
pub mod parser;

#[derive(Debug, Clone)]
pub enum App {
    Server,
    Client,
}

#[derive(Debug, Clone)]
pub struct FileConfig {
    pub app: App,
    pub server_tcp_port: String,
    pub server_tls_port: Option<String>,
    pub cert_path: Option<String>,
    pub key_path: Option<String>,
    pub server_workers: Option<usize>,
    pub user: Option<String>,
    pub daemonize: bool,
    pub use_websocket: bool,
    pub use_tls: bool,
    pub client_use_tls: bool,
    pub client_use_websocket: bool,
    pub client_thread_count: usize,
    pub protocol_version: Option<u32>, //TODO None for latest, Some(3) for v0.3
    pub logger: LevelFilter,
    pub secret_key: Option<String>,
    pub x_nettest_client: String,
    pub control_server: String,
    pub server_registration: bool,
    pub hostname: Option<String>,
    pub registration_token: Option<String>,
    pub client_uuid: Option<String>,
}

impl Default for FileConfig {
    fn default() -> Self {
        Self {
            app: App::Server,
            server_tcp_port: "5005".to_string(),
            server_tls_port: None,
            cert_path: None,
            key_path: None,
            server_workers: None,
            user: None,
            daemonize: false,
            use_websocket: false,
            use_tls: false,
            protocol_version: None, 
            logger: LevelFilter::Off,
            client_use_tls: false,
            client_use_websocket: false,
            client_thread_count: 3,
            secret_key: None,
            x_nettest_client: "nt".to_string(),
            control_server: "https://api.nettest.org".to_string(),
            server_registration: false,
            hostname: None,
            registration_token: None,
            client_uuid: None,
        }
    }
}
