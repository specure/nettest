use anyhow::Result;
use log::info;
use mio::Token;
use crate::state::TestPhase;

pub struct GreetingHandler {
    token: Token,
    token_sent: bool,
}

impl GreetingHandler {
    pub fn new(token: Token) -> Self {
        Self {
            token,
            token_sent: false,
        }
    }

    pub fn get_token_command(&self) -> String {
        "TOKEN 629786f4-9e17-4e9c-8c1c-7c1c7c1c7c1c_1234567890_abcdef\n".to_string()
    }

    pub fn mark_token_sent(&mut self) {
        self.token_sent = true;
    }

    pub fn handle(&mut self, response: &str) -> Result<Option<TestPhase>> {
        info!("Got greeting: {}", response);
        if response.contains("RMBTv") {
            Ok(Some(TestPhase::TestTokenResponseReceive))
        } else {
            Ok(None)
        }
    }
} 