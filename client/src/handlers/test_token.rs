use anyhow::Result;
use log::debug;
use mio::Token;
use crate::state::TestPhase;

pub struct TestTokenHandler {
    token: Token,
    pub token_sent: bool,
}

impl TestTokenHandler {
    pub fn new(token: Token) -> Self {
        Self { token, token_sent: false }
    }

    pub fn handle(&mut self, response: &str) -> Result<Option<TestPhase>> {
        if response.contains("OK") {
            debug!("Token accepted");
            Ok(Some(TestPhase::GetChunksReceive))
        } else {
            Ok(None)
        }
    }

    pub fn get_token_command(&self) -> &'static str {
        "TEST_TOKEN\n"
    }

    pub fn mark_token_sent(&mut self) {
        self.token_sent = true;
    }
} 