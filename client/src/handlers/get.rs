use anyhow::Result;
use log::debug;
use mio::{net::TcpStream, Interest, Token};
use crate::state::TestPhase;

pub struct GetHandler {
    stream: TcpStream,
    token: Token,
    chunk_size: u32,
}

impl GetHandler {
    pub fn new(stream: TcpStream, token: Token, chunk_size: u32) -> Self {
        Self {
            stream,
            token,
            chunk_size,
        }
    }

    pub fn handle(&mut self, response: &str) -> Result<Option<TestPhase>> {
        if response.contains("OK") {
            debug!("GET test completed");
            Ok(Some(TestPhase::GetNoResult))
        } else {
            Ok(None)
        }
    }

    pub fn get_get_command(&self) -> String {
        format!("GET {}\n", self.chunk_size)
    }
} 