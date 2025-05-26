use anyhow::Result;
use log::debug;
use mio::Token;
use std::time::Instant;
use crate::state::TestPhase;

pub struct GetChunksHandler {
    token: Token,
    pub chunks_received: usize,
    pub total_chunks: usize,
    pub chunk_size: u32,
    pub test_start_time: Option<Instant>,
}

impl GetChunksHandler {
    pub fn new(token: Token) -> Self {
        Self {
            token,
            chunks_received: 0,
            total_chunks: 8, // Начинаем с 8 чанков, как указано в спецификации
            chunk_size: 4096, // Начальный размер чанка
            test_start_time: Some(Instant::now()),
        }
    }

    pub fn handle(&mut self, response: &[u8], n: usize) -> Result<Option<TestPhase>> {
        if response.len() >= self.chunk_size as usize {
            self.chunks_received += 1;
            debug!("Received chunk {}/{}", self.chunks_received, self.total_chunks);
            
            // Проверяем терминальный байт (последний байт чанка)
            let last_byte = response[response.len() - 1];
            if last_byte == 0xFF {
                debug!("Received termination byte, all chunks received");
                return Ok(Some(TestPhase::GetChunksProcess));
            }
        }
        Ok(None)
    }

    pub fn get_chunks_command(&self) -> String {
        format!("GETCHUNKS {} {}\n", self.total_chunks, self.chunk_size)
    }

    pub fn get_ok_command(&self) -> &'static str {
        "OK\n"
    }

    pub fn get_bytes_per_sec(&self) -> Vec<f64> {
        vec![] // TODO: Implement actual measurement
    }
} 