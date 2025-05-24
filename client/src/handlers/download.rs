use crate::stream::Stream;
use anyhow::Result;
use log::{debug, info};
use std::time::Instant;

const MIN_CHUNK_SIZE: u32 = 4096; // 4KB
const MAX_CHUNK_SIZE: u32 = 4194304; // 4MB
const PRE_DOWNLOAD_DURATION_NS: u64 = 2_000_000_000; // 2 seconds
const MESSAGES_PER_10GB: u32 = 5_000_000;

pub struct GetChunksHandler<'a> {
    stream: &'a mut Stream,
    pre_download_end_time: u64,
    pre_download_bytes_read: u64,
    is_chunk_portion_finished: bool,
    chunk_messages: Vec<String>,
    current_chunk_size: u32,
    bytes_per_sec_pre_download: Vec<f64>,
}

impl<'a> GetChunksHandler<'a> {

    pub async fn run(&mut self) -> Result<u32> {
        // Start pre-download phase
        self.start_pre_download().await?;
        
        // Calculate optimal chunk size based on pre-download results
        let optimal_chunk_size = self.calculate_chunk_size();
        debug!("Calculated optimal chunk size: {}", optimal_chunk_size);
        
        // Run main download with optimal chunk size
        self.run_download(optimal_chunk_size).await?;
        
        Ok(optimal_chunk_size)
    }

    async fn start_pre_download(&mut self) -> Result<()> {
        let mut chunk_messages = Vec::new();
        let mut chunk_size = 1;
        
        // Generate chunk messages for different sizes (1, 2, 4, 8, 16, etc.)
        while chunk_size <= MAX_CHUNK_SIZE {
            chunk_messages.push(format!("GETCHUNKS {}\n", chunk_size));
            chunk_size *= 2;
        }

        let start_time = Instant::now();
        self.pre_download_end_time = start_time.elapsed().as_nanos() as u64 + PRE_DOWNLOAD_DURATION_NS;
        self.current_chunk_size = MIN_CHUNK_SIZE;
        
        loop {
            let elapsed = start_time.elapsed().as_nanos() as u64;
            if elapsed >= self.pre_download_end_time {
                break;
            }
            
            // Send GETCHUNKS command
            let command = format!("GETCHUNKS {}\n", self.current_chunk_size);
            debug!("Sending pre-download GETCHUNKS command: {}", command);
            self.stream.write_all(command.as_bytes()).await?;
            
            // Read response
            let mut buffer = vec![0u8; self.current_chunk_size as usize];
            let n = self.stream.read(&mut buffer).await?;
            
            if n == 0 {
                return Err(anyhow::anyhow!("Connection closed during pre-download"));
            }
            
            self.pre_download_bytes_read += n as u64;
            
            // Check for ACCEPT_GETCHUNKS response
            if buffer.starts_with(b"ACCEPT_GETCHUNKS") {
                self.current_chunk_size *= 2;
                if self.current_chunk_size > MAX_CHUNK_SIZE {
                    self.current_chunk_size = MAX_CHUNK_SIZE;
                }
            }
            
            if buffer.starts_with(b"TIME") {
                let time_ns = String::from_utf8_lossy(&buffer[5..])
                    .trim()
                    .parse::<u64>()?;
                let bytes_per_sec = self.pre_download_bytes_read as f64 / (time_ns as f64 / 1e9);
                self.bytes_per_sec_pre_download.push(bytes_per_sec);
            }
            self.stream.write_all(b"OK").await?;
        }
        
        Ok(())
    }

    fn calculate_chunk_size(&self) -> u32 {
        let bytes_per_sec_total: f64 = self.bytes_per_sec_pre_download.iter().sum();
        let avg_bytes_per_sec = bytes_per_sec_total / self.bytes_per_sec_pre_download.len() as f64;
        
        // Calculate chunk size to get 1 chunk every 20ms on average
        let mut chunk_size = (avg_bytes_per_sec / (1000.0 / 20.0)) as u32;
        
        // Ensure chunk size is within bounds
        chunk_size = chunk_size.max(MIN_CHUNK_SIZE).min(MAX_CHUNK_SIZE);
        
        debug!("Calculated chunk size: {} bytes", chunk_size);
        chunk_size
    }

    async fn run_download(&mut self, chunk_size: u32) -> Result<()> {
        let command = format!("GETTIME {}\n", chunk_size);
        debug!("Starting main download with chunk size: {}", chunk_size);
        self.stream.write_all(command.as_bytes()).await?;
        self.stream.flush().await?;
        
        let mut total_bytes = 0;
        let mut found_terminator = false;
        
        while !found_terminator {
            let mut buffer = vec![0u8; chunk_size as usize];
            let n = self.stream.read(&mut buffer).await?;
            
            if n == 0 {
                return Err(anyhow::anyhow!("Connection closed during download"));
            }
            
            total_bytes += n;
            let last_byte = buffer[n - 1];
            
            debug!(
                "Read {} bytes, total: {}, last byte: 0x{:02X}",
                n, total_bytes, last_byte
            );
            
            if last_byte == 0xFF {
                found_terminator = true;
                debug!("Found terminator byte 0xFF, download complete");
            }
        }
        
        // Send OK after receiving all chunks
        self.stream.write_all(b"OK").await?;
        self.stream.flush().await?;
        
        // Read TIME response
        let mut response = [0u8; 1024];
        let n = self.stream.read(&mut response).await?;
        let time_response = String::from_utf8_lossy(&response[..n]);
        debug!("Received TIME response: {}", time_response);
        
        Ok(())
    }
}

pub async fn handle_get_chunks(
    stream: &mut Stream,
) -> Result<()> {
    let mut handler = GetChunksHandler::new(stream);
    handler.run().await?;
    Ok(())
}
