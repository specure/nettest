use std::error::Error;
use std::sync::Arc;
use tokio::sync::Mutex;
use crate::config::constants::{RESP_OK, RESP_ERR, RESP_BYE, RESP_PONG, RESP_TIME, CHUNK_SIZE, MAX_CHUNKS, MAX_PUT_SIZE};
use crate::server::connection_handler::Stream;

pub async fn handle_get_time(stream: &mut Stream) -> Result<(), Box<dyn Error + Send + Sync>> {
    let time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    stream.write_all(format!("TIME {}\n", time).as_bytes()).await?;
    Ok(())
}

pub async fn handle_get_chunks(stream: &mut Stream, data_buffer: Arc<Mutex<Vec<u8>>>) -> Result<(), Box<dyn Error + Send + Sync>> {
    stream.write_all(RESP_OK.as_bytes()).await?;
    
    let data = data_buffer.lock().await;
    let total_chunks = (data.len() + CHUNK_SIZE - 1) / CHUNK_SIZE;
    
    if total_chunks > MAX_CHUNKS {
        return Err("Too many chunks".into());
    }
    
    for chunk in data.chunks(CHUNK_SIZE) {
        stream.write_all(chunk).await?;
    }
    
    Ok(())
}

pub async fn handle_put(stream: &mut Stream, data_buffer: Arc<Mutex<Vec<u8>>>) -> Result<(), Box<dyn Error + Send + Sync>> {
    stream.write_all(RESP_OK.as_bytes()).await?;
    
    let mut data = Vec::new();
    loop {
        let mut buf = [0u8; CHUNK_SIZE];
        let n = stream.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        data.extend_from_slice(&buf[..n]);
    }
    
    let mut buffer = data_buffer.lock().await;
    *buffer = data;
    
    Ok(())
}

pub async fn handle_put_no_result(stream: &mut Stream, data_buffer: Arc<Mutex<Vec<u8>>>) -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut data = Vec::new();
    loop {
        let mut buf = [0u8; CHUNK_SIZE];
        let n = stream.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        data.extend_from_slice(&buf[..n]);
    }
    
    let mut buffer = data_buffer.lock().await;
    *buffer = data;
    
    Ok(())
}

pub async fn handle_ping(stream: &mut Stream) -> Result<(), Box<dyn Error + Send + Sync>> {
    stream.write_all(RESP_PONG.as_bytes()).await?;
    Ok(())
}

pub async fn handle_quit(stream: &mut Stream) -> Result<(), Box<dyn Error + Send + Sync>> {
    stream.write_all(RESP_BYE.as_bytes()).await?;
    Ok(())
} 