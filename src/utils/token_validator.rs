use std::error::Error;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::time::sleep;
use log::{debug, error, info};
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
    pub async fn validate(&self, token_uuid: &str, start_time_str: &str, hmac: &str) -> Result<bool, Box<dyn Error + Send + Sync>> {
        // Проверяем токен с каждым ключом
        for (i, key) in self.secret_keys.iter().enumerate() {
            if self.validate_with_key(token_uuid, start_time_str, hmac, key).await? {
                info!("Token was accepted by key {}", self.secret_keys_labels[i]);
                debug!("Token was accepted by key {}", self.secret_keys[i]);
                return Ok(true);
            }
        }
        
        error!("Got illegal token: \"{}\"", token_uuid);
        Ok(false)
    }

    async fn validate_with_key(&self, token_uuid: &str, start_time_str: &str, hmac: &str, key: &str) -> Result<bool, Box<dyn Error + Send + Sync>> {
        // Проверяем формат UUID
        if Uuid::parse_str(token_uuid).is_err() {
            error!("Invalid UUID format: \"{}\"", token_uuid);
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
        let message = format!("{}_{}", token_uuid, start_time_str);
        let mut mac = HmacSha1::new_from_slice(key.as_bytes())?;
        mac.update(message.as_bytes());
        let result = mac.finalize();
        let code_bytes = result.into_bytes();
        let computed_hmac = BASE64.encode(code_bytes);

        Ok(computed_hmac == hmac)
    }

    /// Генерация HMAC для токена
    pub fn generate_hmac(token_uuid: &str, start_time_str: &str, key: &str) -> Result<String, Box<dyn Error + Send + Sync>> {
        let message = format!("{}_{}", token_uuid, start_time_str);
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

    const TEST_KEY_1: &str = "q4aFShYnBgoYyDr4cxes0DSYCvjLpeKJjhCfvmVCdiIpsdeU1djvBtE6CMtNCbDWkiU68X7bajIAwLon14Hh7Wpi5MJWJL7HXokh";
    const TEST_KEY_2: &str = "test_key_1234567892";
    const TEST_LABEL_1: &str = "auto-generated key";
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

        let token = format!("{}_{}_{}", uuid, start_time, hmac);

        println!("Generated token: {}", token);
        
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
        let validator = TokenValidator::new(vec!["test_key".to_string()], vec!["test_label".to_string()]);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        
        // Создаем токен, который начнется через 2 секунды
        let future_time = (now + 2).to_string();
        let uuid = Uuid::new_v4().to_string();
        let hmac = TokenValidator::generate_hmac(&uuid, &future_time, "test_key")
            .expect("Failed to generate HMAC");
        
        let result = validator.validate(&uuid, &future_time, &hmac).await;
        assert!(result.is_ok());
        assert!(result.unwrap());
    }

    #[tokio::test]
    async fn test_token_too_late() {
        let validator = TokenValidator::new(vec!["test_key".to_string()], vec!["test_label".to_string()]);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        
        // Создаем токен, который уже просрочен на 91 секунду (больше чем MAX_ACCEPT_LATE)
        let past_time = (now - 91).to_string();
        let uuid = Uuid::new_v4().to_string();
        let hmac = TokenValidator::generate_hmac(&uuid, &past_time, "test_key")
            .expect("Failed to generate HMAC");
        
        let result = validator.validate(&uuid, &past_time, &hmac).await;
        assert!(result.is_ok());
        assert!(!result.unwrap());
    }
}
