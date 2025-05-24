// use crate::stream::{Stream, TcpStream};
// use std::io::{self, Read, Write};
// use std::net::SocketAddr;
// use std::time::Duration;
// use log::{info, error};
// use tokio::time::timeout;

//     pub async fn connect_rmbt() -> io::Result<u32> {
//         // Send RMBT upgrade request


//         // Read and verify upgrade response with timeout
//         info!("Waiting for upgrade response...");
//         let n = timeout(Duration::from_secs(5), self.stream.read(&mut self.buffer)).await??;
//         let response_str = String::from_utf8_lossy(&self.buffer[..n]);
//         info!("Received RMBT upgrade response: {}", response_str);

//         if !response_str.contains("Upgrade: RMBT") {
//             return Err(io::Error::new(io::ErrorKind::InvalidData, "Invalid upgrade response"));
//         }

//         // Read greeting message with timeout
//         info!("Waiting for greeting message...");
//         let n = timeout(Duration::from_secs(5), self.stream.read(&mut self.buffer)).await??;
//         let greeting_str = String::from_utf8_lossy(&self.buffer[..n]);
//         info!("Received greeting: {}", greeting_str);
//         if !greeting_str.contains("RMBTv") {
//             return Err(io::Error::new(io::ErrorKind::InvalidData, "Invalid greeting"));
//         }

//         // Read ACCEPT TOKEN message with timeout
//         info!("Waiting for ACCEPT TOKEN message...");
//         let n = timeout(Duration::from_secs(5), self.stream.read(&mut self.buffer)).await??;
//         let accept_token_str = String::from_utf8_lossy(&self.buffer[..n]);
//         info!("Received ACCEPT TOKEN message: {}", accept_token_str);
//         if !accept_token_str.contains("ACCEPT TOKEN") {
//             return Err(io::Error::new(io::ErrorKind::InvalidData, "Invalid ACCEPT TOKEN message"));
//         }

//         // Send token
//         let token = "TEST_TOKEN";  
//         info!("Sending token: {}", token);
//         self.stream.write_all(token.as_bytes()).await?;

//         // Read token response with timeout
//         info!("Waiting for token response...");
//         let n = timeout(Duration::from_secs(5), self.stream.read(&mut self.buffer)).await??;
//         let response_str = String::from_utf8_lossy(&self.buffer[..n]);
//         info!("Received token response: {}", response_str);
//         if !response_str.contains("OK") {
//             return Err(io::Error::new(io::ErrorKind::InvalidData, "Token not accepted"));
//         }

//         // Read CHUNKSIZE message with timeout
//         info!("Waiting for CHUNKSIZE message...");
//         let n = timeout(Duration::from_secs(5), self.stream.read(&mut self.buffer)).await??;
//         let chunksize_str = String::from_utf8_lossy(&self.buffer[..n]);
//         info!("Received CHUNKSIZE message: {}", chunksize_str);
//         if !chunksize_str.contains("CHUNKSIZE") {
//             return Err(io::Error::new(io::ErrorKind::InvalidData, "Invalid CHUNKSIZE message"));
//         }

//         // Parse chunk size
//         let chunk_size = chunksize_str
//             .split_whitespace()
//             .nth(1)
//             .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Failed to parse chunk size"))?
//             .parse::<u32>()
//             .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

//         Ok(chunk_size)
//     }
