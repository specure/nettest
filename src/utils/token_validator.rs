use std::error::Error;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::time::sleep;
use log::{error, info, debug};
use crate::config::constants::{MAX_ACCEPT_EARLY, MAX_ACCEPT_LATE};
use hmac::{Hmac, Mac};
use sha1::Sha1;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use uuid::Uuid;

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
        // Проверяем формат UUID
        if Uuid::parse_str(uuid).is_err() {
            error!("Invalid UUID format: \"{}\"", uuid);
            return Ok(false);
        }

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

    /// Генерация HMAC для токена
    pub fn generate_hmac(uuid: &str, start_time_str: &str, key: &str) -> Result<String, Box<dyn Error + Send + Sync>> {
        let message = format!("{}_{}", uuid, start_time_str);
        let mut mac = HmacSha1::new_from_slice(key.as_bytes())?;
        mac.update(message.as_bytes());
        let result = mac.finalize();
        let code_bytes = result.into_bytes();
        Ok(BASE64.encode(code_bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    const TEST_KEY_1: &str = "test_key_1234567890";
    const TEST_KEY_2: &str = "test_key_1234567892";
    const TEST_LABEL_1: &str = "test_label_1";
    const TEST_LABEL_2: &str = "test_label_2";

    fn create_test_validator() -> TokenValidator {
        let secret_keys = vec![TEST_KEY_1.to_string()];
        let secret_keys_labels = vec![TEST_LABEL_1.to_string()];
        TokenValidator::new(secret_keys, secret_keys_labels)
    }

    fn create_test_validator_with_two_keys() -> TokenValidator {
        let secret_keys = vec![TEST_KEY_1.to_string(), TEST_KEY_2.to_string()];
        let secret_keys_labels = vec![TEST_LABEL_1.to_string(), TEST_LABEL_2.to_string()];
        TokenValidator::new(secret_keys, secret_keys_labels)
    }

    #[tokio::test]
    async fn test_valid_token() {
        let validator = create_test_validator();
        let uuid = Uuid::new_v4().to_string();
        let start_time = (SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64)
            .to_string();
        
        // Генерируем правильный HMAC для тестового ключа
        let hmac = TokenValidator::generate_hmac(&uuid, &start_time, TEST_KEY_1)
            .expect("Failed to generate HMAC");
        
        let result = validator.validate(&uuid, &start_time, &hmac).await;
        assert!(result.is_ok());
        assert!(result.unwrap());
    }

    #[tokio::test]
    async fn test_token_signed_with_different_key() {
        let validator = create_test_validator();
        let uuid = Uuid::new_v4().to_string();
        let start_time = (SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64)
            .to_string();
        
        // Генерируем HMAC с другим ключом
        let hmac = TokenValidator::generate_hmac(&uuid, &start_time, TEST_KEY_2)
            .expect("Failed to generate HMAC");
        
        let result = validator.validate(&uuid, &start_time, &hmac).await;
        assert!(result.is_ok());
        assert!(!result.unwrap());
    }

    #[tokio::test]
    async fn test_token_validated_with_multiple_keys() {
        let validator = create_test_validator_with_two_keys();
        let uuid = Uuid::new_v4().to_string();
        let start_time = (SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64)
            .to_string();
        
        // Генерируем HMAC с первым ключом
        let hmac = TokenValidator::generate_hmac(&uuid, &start_time, TEST_KEY_1)
            .expect("Failed to generate HMAC");
        
        let result = validator.validate(&uuid, &start_time, &hmac).await;
        assert!(result.is_ok());
        assert!(result.unwrap());

        // Генерируем HMAC со вторым ключом
        let hmac = TokenValidator::generate_hmac(&uuid, &start_time, TEST_KEY_2)
            .expect("Failed to generate HMAC");
        
        let result = validator.validate(&uuid, &start_time, &hmac).await;
        assert!(result.is_ok());
        assert!(result.unwrap());
    }

    #[tokio::test]
    async fn test_invalid_uuid_format() {
        let validator = create_test_validator();
        let invalid_uuid = "not-a-uuid";
        let start_time = "1234567890";
        let hmac = TokenValidator::generate_hmac(invalid_uuid, start_time, TEST_KEY_1)
            .expect("Failed to generate HMAC");
        
        let result = validator.validate(invalid_uuid, start_time, &hmac).await;
        assert!(result.is_ok());
        assert!(!result.unwrap());
    }

    #[tokio::test]
    async fn test_token_too_early() {
        let validator = create_test_validator();
        let uuid = Uuid::new_v4().to_string();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        // Создаем токен, который начнет действовать через 2 секунды
        let future_time = (now + 2).to_string();
        println!("Current time: {}, Future time: {}", now, future_time);
        let hmac = TokenValidator::generate_hmac(&uuid, &future_time, TEST_KEY_1)
            .expect("Failed to generate HMAC");
        
        let start = SystemTime::now();
        let result = validator.validate(&uuid, &future_time, &hmac).await;
        let duration = start.elapsed().unwrap();
        println!("Test too early result: {:?}, waited for: {:?}", result, duration);
        assert!(result.is_ok());
        assert!(result.unwrap());
        // Проверяем, что мы ждали примерно 2 секунды
        assert!(duration.as_secs() >= 1 && duration.as_secs() <= 3);
    }

    #[tokio::test]
    async fn test_token_too_late() {
        let validator = create_test_validator();
        let uuid = Uuid::new_v4().to_string();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let past_time = (now - MAX_ACCEPT_LATE as i64 - 1).to_string();
        println!("Current time: {}, Past time: {}", now, past_time);
        let hmac = TokenValidator::generate_hmac(&uuid, &past_time, TEST_KEY_1)
            .expect("Failed to generate HMAC");
        
        let result = validator.validate(&uuid, &past_time, &hmac).await;
        println!("Test too late result: {:?}", result);
        assert!(result.is_ok());
        assert!(!result.unwrap());
    }
}
