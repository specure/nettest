use crate::client::calculator::calculate_speed_from_measurements;
use crate::client::client::SharedStats;
use log::{warn, info};
use serde_json::json;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;
use std::fs;
use std::path::PathBuf;
use std::env;

pub struct MeasurementSaver {
    control_server_url: String,
    client_uuid: Option<String>,
}

impl MeasurementSaver {
    pub fn new(control_server_url: String, client_uuid: Option<String>) -> Self {
        Self {
            control_server_url,
            client_uuid,
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

    pub async fn save_measurement(
        &self,
        stats: &Mutex<SharedStats>,
        ping_median: Option<u64>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Если client_uuid не задан, не сохраняем
        let client_uuid = match &self.client_uuid {
            Some(uuid) => uuid.clone(),
            None => {
                warn!("Client UUID not configured, skipping measurement save");
                return Ok(());
            }
        };

        let stats_guard = stats.lock().unwrap();
        
        // Вычисляем скорости используя calculate_speed_from_measurements
        let download_speed = if !stats_guard.download_measurements.is_empty() {
            let (_, _, speed_mbps) = calculate_speed_from_measurements(stats_guard.download_measurements.clone());
            Some((speed_mbps * 1_000_000.0) as i32) // Конвертируем в bps как Integer
        } else {
            None
        };

        let upload_speed = if !stats_guard.upload_measurements.is_empty() {
            let (_, _, speed_mbps) = calculate_speed_from_measurements(stats_guard.upload_measurements.clone());
            Some((speed_mbps * 1_000_000.0) as i32) // Конвертируем в bps как Integer
        } else {
            None
        };

        // Конвертируем ping в миллисекунды
        let ping_median_ms = ping_median.map(|ping_ns| (ping_ns as f64 / 1_000_000.0) as i64);

        // Генерируем openTestUuid
        let open_test_uuid = Uuid::new_v4().to_string();

        // Получаем текущее время
        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Формируем данные для отправки
        let measurement_data = json!({
            "openTestUuid": open_test_uuid,
            "clientUuid": client_uuid,
            "speedDownload": download_speed,
            "speedUpload": upload_speed,
            "pingMedian": ping_median_ms,
            "time": current_time,
            "clientVersion": env!("CARGO_PKG_VERSION")
        });

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

    pub async fn save_measurement_with_speeds(
        &mut self,
        ping_median: Option<u64>,
        download_speed_gbps: Option<f64>,
        upload_speed_gbps: Option<f64>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Обеспечиваем наличие client_uuid
        let client_uuid = self.ensure_client_uuid()?;

        // Конвертируем скорости из Gbps в bps
        let download_speed = download_speed_gbps.map(|speed| (speed * 1_000_000_000.0) as i32);
        let upload_speed = upload_speed_gbps.map(|speed| (speed * 1_000_000_000.0) as i32);

        // Конвертируем ping в миллисекунды
        let ping_median_ms = ping_median.map(|ping_ns| (ping_ns as f64 / 1_000_000.0) as i64);

        // Генерируем openTestUuid
        let open_test_uuid = Uuid::new_v4().to_string();

        // Получаем текущее время
        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Формируем данные для отправки
        let measurement_data = json!({
            "openTestUuid": open_test_uuid,
            "clientUuid": client_uuid,
            "speedDownload": download_speed,
            "speedUpload": upload_speed,
            "pingMedian": ping_median_ms,
            "time": current_time,
            "clientVersion": env!("CARGO_PKG_VERSION")
        });

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
