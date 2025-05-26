use anyhow::Result;
use log::debug;
use mio::{net::TcpStream, Interest, Token};
use crate::state::TestPhase;

pub struct GetNoResultHandler {
    stream: TcpStream,
    token: Token,
    chunk_size: u32,
}

impl GetNoResultHandler {
    pub fn new(stream: TcpStream, token: Token, chunk_size: u32) -> Self {
        Self {
            stream,
            token,
            chunk_size,
        }
    }

    pub fn handle(&mut self, response: &str) -> Result<Option<TestPhase>> {
        if response.contains("OK") {
            debug!("GET_NO_RESULT test completed");
            Ok(Some(TestPhase::End))
        } else {
            Ok(None)
        }
    }

    pub fn get_get_no_result_command(&self) -> String {
        format!("GET_NO_RESULT {}\n", self.chunk_size)
    }
} 