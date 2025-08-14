use crate::client::client::{SharedStats, ClientConfig};
use log::{warn, info};
use serde_json::json;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;
use std::fs;
use std::path::PathBuf;
use std::env;

#[derive(Debug, Clone, Copy)]
pub enum ConnectionType {
    TCP,
    TLS,
    WS,
    WSS,
}

impl ConnectionType {
    fn as_str(&self) -> &'static str {
        match self {
            ConnectionType::TCP => "TCP",
            ConnectionType::TLS => "TLS",
            ConnectionType::WS => "WS",
            ConnectionType::WSS => "WSS",
        }
    }
}

pub struct MeasurementSaver {
    control_server_url: String,
    client_uuid: Option<String>,
    connection_type: ConnectionType,
    threads_number: u32,
    git_hash: Option<String>,
}

impl MeasurementSaver {
    pub fn new(control_server_url: String, client_config: &ClientConfig) -> Self {
        // Определяем тип соединения на основе конфигурации
        let connection_type = if client_config.use_websocket {
            if client_config.use_tls {
                ConnectionType::WSS
            } else {
                ConnectionType::WS
            }
        } else if client_config.use_tls {
            ConnectionType::TLS
        } else {
            ConnectionType::TCP
        };

        Self {
            control_server_url,
            client_uuid: client_config.client_uuid.clone(),
            connection_type,
            threads_number: client_config.thread_count as u32,
            git_hash: client_config.git_hash.clone(),
        }
    }

    fn ensure_client_uuid(&mut self) -> Result<String, Box<dyn std::error::Error>> {
        // Если client_uuid уже есть, возвращаем его
        if let Some(uuid) = &self.client_uuid {
            return Ok(uuid.clone());
        }
        
        // Генерируем новый UUID
        let new_uuid = Uuid::new_v4().to_string();
        
        // Определяем путь к конфигурационному файлу
        let config_path = if cfg!(target_os = "macos") {
            let home_dir = env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
            PathBuf::from(format!("{}/.config/nettest.conf", home_dir))
        } else {
            PathBuf::from("/etc/nettest.conf")
        };
        
        // Читаем текущий контент файла
        let mut content = if config_path.exists() {
            fs::read_to_string(&config_path)?
        } else {
            String::new()
        };
        
        // Проверяем, есть ли уже client_uuid (независимо от того, закомментирован он или нет)
        let has_uuid = content.lines().any(|line| {
            let line = line.trim();
            line.starts_with("client_uuid") && !line.starts_with("#")
        });
        
        // Если client_uuid нет или он закомментирован, добавляем его
        if !has_uuid {
            if !content.is_empty() && !content.ends_with('\n') {
                content.push('\n');
            }
            content.push_str(&format!("client_uuid=\"{}\"\n", new_uuid));
            
            // Записываем обновленный контент
            fs::write(&config_path, content)?;
            println!("Generated and saved new client UUID: {}", new_uuid);
        }
        
        // Обновляем внутреннее состояние
        self.client_uuid = Some(new_uuid.clone());
        
        Ok(new_uuid)
    }

    pub async fn save_measurement_with_speeds(
        &mut self,
        ping_median: Option<u64>,
        download_speed_gbps: Option<f64>,
        upload_speed_gbps: Option<f64>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Обеспечиваем наличие client_uuid
        let client_uuid = self.ensure_client_uuid()?;

        // Конвертируем скорости из Gbps в сотые доли Mbps (например: 57.3 Gbps -> 5730)
        let download_speed = download_speed_gbps.map(|speed| (speed * 100.0) as i32);
        let upload_speed = upload_speed_gbps.map(|speed| (speed * 100.0) as i32);

        // Сохраняем ping в наносекундах для большей точности
        let ping_median_ns = ping_median;

        // Генерируем openTestUuid - используем GITHUB_SHA если есть, иначе генерируем
        let open_test_uuid = Uuid::new_v4().to_string();

        // Получаем текущее время
        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Формируем данные для отправки
        let mut measurement_data = json!({
            "openTestUuid": open_test_uuid,
            "clientUuid": client_uuid,
            "speedDownload": download_speed,
            "speedUpload": upload_speed,
            "pingMedian": ping_median_ns,
            "time": current_time,
            "clientVersion": "2.0.0",
            "connectionType": self.connection_type.as_str(),
            "threadsNumber": self.threads_number,
        });

        // Добавляем commitHash только если есть git_hash в конфигурации
        if let Some(git_hash) = &self.git_hash {
            measurement_data["commitHash"] = json!(git_hash);
        } else if let Ok(commit_hash) = std::env::var("GITHUB_SHA") {
            measurement_data["commitHash"] = json!(commit_hash);
        }

        info!("Final measurement data: {:?}", measurement_data);

        info!("Saving measurement: {:?}", measurement_data);

        // Отправляем POST запрос
        let client = reqwest::Client::new();
        let response = client
            .post(&format!("{}/measurement/save", self.control_server_url))
            .header("Content-Type", "application/json")
            .header("x-nettest-client", "nt")
            .json(&measurement_data)
            .send()
            .await?;

        if response.status().is_success() {
            info!("Measurement saved successfully");
        } else {
            warn!("Failed to save measurement: HTTP {}", response.status());
            if let Ok(error_text) = response.text().await {
                warn!("Error response: {}", error_text);
            }
        }

        Ok(())
    }
}
