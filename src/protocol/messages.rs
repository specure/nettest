use std::error::Error;
use crate::server::connection_handler::Stream;

pub async fn read_line(stream: &mut Stream) -> Result<String, Box<dyn Error + Send + Sync>> {
    let mut buf = [0u8; 1024];
    let mut pos = 0;
    
    loop {
        let n = stream.read(&mut buf[pos..]).await?;
        if n == 0 {
            return Err("Connection closed".into());
        }
        
        pos += n;
        if buf[..pos].contains(&b'\n') {
            return Ok(String::from_utf8_lossy(&buf[..pos-1]).to_string());
        }
    }
} 