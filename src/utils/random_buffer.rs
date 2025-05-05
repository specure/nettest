use once_cell::sync::Lazy;
use std::sync::Arc;
use std::ops::Range;

const RANDOM_SIZE: usize = 100 * 1024 * 1024; // 100MB

pub static RANDOM_BUFFER: Lazy<Arc<Vec<u8>>> = Lazy::new(|| {
    let mut buf = vec![0u8; RANDOM_SIZE];
    fastrand::fill(&mut buf);
    log::info!("Random buffer created!");
    Arc::new(buf)
});

pub fn get_random_slice(range: Range<usize>) -> Vec<u8> {
    let buf = &RANDOM_BUFFER;
    let mut result = Vec::with_capacity(range.len());
    let len = buf.len();
    for i in range {
        result.push(buf[i % len]);
    }
    result
} 