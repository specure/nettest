use std::sync::Arc;

const RANDOM_SIZE: usize = 100 * 1024 * 1024; // 100MB

static mut RANDOM_BUFFER: Option<Arc<Vec<u8>>> = None;

pub fn init_random_buffer() {
    let mut buf = vec![0u8; RANDOM_SIZE];
    fastrand::fill(&mut buf);
    log::debug!("Random buffer created!");
    unsafe {
        RANDOM_BUFFER = Some(Arc::new(buf));
    }
}

pub fn get_buffer_size() -> usize {
    return RANDOM_SIZE;
}

pub fn get_random_buffer() -> Arc<Vec<u8>> {
    unsafe {
        RANDOM_BUFFER.as_ref().expect("Random buffer not initialized").clone()
    }
}

pub fn get_random_slice(buffer: &mut [u8], offset: usize) -> usize {
    let random_buf = get_random_buffer();
    let start = offset % RANDOM_SIZE;
    
    if start + buffer.len() <= RANDOM_SIZE {
        buffer.copy_from_slice(&random_buf[start..start + buffer.len()]);
        return start + RANDOM_SIZE
    } else {
        buffer.copy_from_slice(&random_buf[..buffer.len()]);
        return  RANDOM_SIZE
    }
}