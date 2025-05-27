use anyhow::Result;
use log::debug;
use mio::{net::TcpStream, Interest, Token};
use crate::state::TestPhase;

pub struct PutNoResultHandler {
    stream: TcpStream,
    token: Token,
    chunk_size: u32,
}

impl PutNoResultHandler {
    pub fn new(stream: TcpStream, token: Token, chunk_size: u32) -> Self {
        Self {
            stream,
            token,
            chunk_size,
        }
    }

    pub fn handle(&mut self, response: &str) -> Result<Option<TestPhase>> {
        // if response.contains("OK") {
        //     debug!("PUT_NO_RESULT test completed");
        //     Ok(Some(TestPhase::Get))
        // } else {
            Ok(None)
        // }
    }

    pub fn get_put_no_result_command(&self) -> String {
        format!("PUT_NO_RESULT {}\n", self.chunk_size)
    }
} 