use crate::{config::constants::{CHUNK_SIZE, MAX_CHUNK_SIZE, MIN_CHUNK_SIZE, RESP_ERR}, utils::random_buffer::{CHUNK_STORAGE, CHUNK_TERMINATION_STORAGE}};
use bytes::Bytes;
use std::error::Error;
use std::time::Instant;

use crate::stream::Stream;
use log::{debug, error, info};

const TEST_DURATION: u64 = 10;

pub async fn handle_put_no_result(
    stream: &mut Stream,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let chunk_size = MAX_CHUNK_SIZE as u64;



   
    let command =  format!("PUTNORESULT {}\n", chunk_size);

    debug!("Sending command: {}", command);

    stream.write_all(command.as_bytes()).await?;

    let mut read_buffer: Vec<u8> = vec![0u8; CHUNK_SIZE];
    loop {
        let mut buffer: Vec<u8> = vec![0u8; CHUNK_SIZE];
        let n = stream.read(&mut buffer).await?;
        if n == 0 {
            break;
        }
        read_buffer.extend_from_slice(&buffer[..n]);
        let line = String::from_utf8_lossy(&read_buffer);
        if line.contains("OK\n") {
            break;
        }
    }


    debug!("Received OK response");





    // Parse and validate chunk size


    let mut total_bytes = 0;


    let start_time = Instant::now();
    // Send data until time expires
    while start_time.elapsed().as_secs() < TEST_DURATION {
        // Get next chunk from the array, cycling through all chunks
        // debug!("Sending chunk {}", chunk_index);

        let left = (total_bytes % chunk_size) as usize;

        let remaining = &CHUNK_STORAGE[&chunk_size][left..];


        // let chunk = &CHUNK_STORAGE[&chunk_size];
        let n = stream.write(remaining).await?;
        total_bytes += n as u64;
        // total_bytes += chunk_size;
    }
    debug!("Sending last chunk");
    stream.write_all(&CHUNK_TERMINATION_STORAGE[&chunk_size]).await?;
    stream.flush().await?;
    total_bytes += chunk_size;

    debug!("All data sent. Total bytes: {}", total_bytes);

    // Wait for OK response from client
    let mut response = [0u8; 1024];
    let n = stream.read(&mut response).await?;
    let response_str = String::from_utf8_lossy(&response[..n]);


    info!("All data sent. Total bytes: {} time: {}", total_bytes, response_str);

 

    Ok(())
}
