use bytes::{BytesMut};
use lazy_static::lazy_static;
use std::collections::HashMap;
use std::sync::Arc;

pub const MIN_CHUNK_SIZE: u64 = 4096; // 4KB
pub const MAX_CHUNK_SIZE: u64 = 4194304; // 4MB

lazy_static! {
    pub static ref CHUNK_STORAGE: Arc<HashMap<u64, BytesMut>> = {
        let mut storage = HashMap::new();
        let mut size = MIN_CHUNK_SIZE;
        while size <= MAX_CHUNK_SIZE {
            let mut buffer = BytesMut::with_capacity(size as usize);
            buffer.resize(size as usize, 0);
            fastrand::fill(&mut buffer);
            buffer[size as usize - 1] = 0x00;
            storage.insert(size, buffer);
            size *= 2;
        }
        Arc::new(storage)
    };
    pub static ref CHUNK_TERMINATION_STORAGE: Arc<HashMap<u64, BytesMut>> = {
        let mut storage = HashMap::new();
        let mut size = MIN_CHUNK_SIZE;
        while size <= MAX_CHUNK_SIZE {
            let mut buffer = BytesMut::with_capacity(size as usize);
            buffer.resize(size as usize, 0);
            fastrand::fill(&mut buffer);
            buffer[size as usize - 1] = 0xFF;
            storage.insert(size, buffer);
            size *= 2;
        }
        Arc::new(storage)
    };
}

pub fn get_chunk(size: u64, terminal: bool) -> BytesMut {
    let chunk_ref = CHUNK_STORAGE.get(&MAX_CHUNK_SIZE).unwrap();
    let mut buffer = BytesMut::with_capacity(size as usize);
    buffer.resize(size as usize, 0);
    buffer[0..size as usize].copy_from_slice(&chunk_ref[0..size  as usize]);
    if terminal {
         buffer[size as usize - 1] = 0xFF;
         return buffer
    } else {
        buffer[size as usize - 1] = 0x00;
        return buffer;
    }
}
