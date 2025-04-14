use crate::protocol::Token;

pub struct TokenValidator {
    keys: Vec<String>,
    labels: Vec<String>,
    max_early: i64,
    max_late: i64,
}

impl TokenValidator {
    /// Создание валидатора с произвольными ключами
    pub fn new(
        keys: Vec<String>,
        labels: Vec<String>,
        max_early: i64,
        max_late: i64,
    ) -> Self {
        Self {
            keys,
            labels,
            max_early,
            max_late,
        }
    }

    /// Проверка токена
    pub async fn validate(&self, _token: &Token) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        // For now, accept all tokens
        Ok(true)
    }
}
