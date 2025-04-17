use std::error::Error;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::time::sleep;
use log::{error, info, debug};
use crate::config::constants::{MAX_ACCEPT_EARLY, MAX_ACCEPT_LATE};
use hmac::{Hmac, Mac};
use sha1::Sha1;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};

type HmacSha1 = Hmac<Sha1>;

pub struct TokenValidator {
    secret_keys: Vec<String>,
    secret_keys_labels: Vec<String>,
}

impl TokenValidator {
    /// Создание валидатора с произвольными ключами
    pub fn new(secret_keys: Vec<String>, secret_keys_labels: Vec<String>) -> Self {
        Self {
            secret_keys,
            secret_keys_labels,
        }
    }

    /// Проверка токена
    pub async fn validate(&self, uuid: &str, start_time_str: &str, hmac: &str) -> Result<bool, Box<dyn Error + Send + Sync>> {
        // Проверяем токен с каждым ключом
        for (i, key) in self.secret_keys.iter().enumerate() {
            if self.validate_with_key(uuid, start_time_str, hmac, key).await? {
                info!("Token was accepted by key {}", self.secret_keys_labels[i]);
                return Ok(true);
            }
        }

        error!("Got illegal token: \"{}\"", uuid);
        Ok(false)
    }

    async fn validate_with_key(&self, uuid: &str, start_time_str: &str, hmac: &str, key: &str) -> Result<bool, Box<dyn Error + Send + Sync>> {
        // Проверяем время
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)?
            .as_secs() as i64;
        
        let start_time = start_time_str.parse::<i64>()?;

        // Проверяем, не слишком ли рано или поздно
        if start_time - (MAX_ACCEPT_EARLY as i64) > now {
            error!("Client is not allowed yet. {} seconds too early", start_time - now);
            return Ok(false);
        }
        if start_time + (MAX_ACCEPT_LATE as i64) < now {
            error!("Client is {} seconds too late", now - start_time);
            return Ok(false);
        }

        // Если клиент подключается немного раньше, заставляем его подождать
        if start_time > now {
            let wait_time = start_time - now;
            debug!("Client is {} seconds too early. Let him wait", wait_time);
            sleep(std::time::Duration::from_secs(wait_time as u64)).await;
        }

        // Проверяем HMAC
        let message = format!("{}_{}", uuid, start_time_str);
        let mut mac = HmacSha1::new_from_slice(key.as_bytes())?;
        mac.update(message.as_bytes());
        let result = mac.finalize();
        let code_bytes = result.into_bytes();
        let computed_hmac = BASE64.encode(code_bytes);

        Ok(computed_hmac == hmac)
    }
}
