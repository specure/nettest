use anyhow::Result;
use log::debug;
use mio::{net::TcpStream, Interest, Token};
use crate::state::TestPhase;

pub struct PutHandler {
    stream: TcpStream,
    token: Token,
    chunk_size: u32,
}

impl PutHandler {
    pub fn new(stream: TcpStream, token: Token, chunk_size: u32) -> Self {
        Self {
            stream,
            token,
            chunk_size,
        }
    }

    pub fn handle(&mut self, response: &str) -> Result<Option<TestPhase>> {
        // if response.contains("OK") {
        //     debug!("PUT test completed");
        //     Ok(Some(TestPhase::PutNoResult))
        // } else {
            Ok(None)
        // 
    }

    pub fn get_put_command(&self) -> String {
        format!("PUT {}\n", self.chunk_size)
    }
} 