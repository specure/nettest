use std::error::Error;
use crate::config::constants::{MIN_CHUNK_SIZE, MAX_CHUNK_SIZE};

pub fn validate_chunk_size(size: usize) -> Result<(), Box<dyn Error + Send + Sync>> {
    if size < MIN_CHUNK_SIZE || size > MAX_CHUNK_SIZE {
        return Err(format!(
            "Chunk size {} is out of bounds. Must be between {} and {}",
            size, MIN_CHUNK_SIZE, MAX_CHUNK_SIZE
        ).into());
    }
    Ok(())
} 