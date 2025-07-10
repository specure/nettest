use std::io::{self};
use bytes::{Buf, BytesMut};
use log::debug;

use crate::stream::stream::Stream;



/// Reads from the stream until the specified string is found in the buffer
/// 
/// # Arguments
/// 
/// * `stream` - The TCP stream to read from
/// * `buffer` - The buffer to store read data
/// * `until` - The string to read until
/// 
/// # Returns
/// 
/// * `Ok(true)` if the target string was found
/// * `Ok(false)` if no more data is available (WouldBlock)
/// * `Err` if an error occurred during reading
pub fn read_until(stream: &mut Stream, buffer: &mut BytesMut, until: &str) -> io::Result<bool> {
    let mut temp_buf = vec![0u8; 8096];

    match stream.read(&mut temp_buf) {
        Ok(n) if n > 0 => {
            buffer.extend_from_slice(&temp_buf[..n]);
            let buffer_str = String::from_utf8_lossy(buffer);
            if buffer_str.contains(until) {
                Ok(true)
            } else {
                Ok(false)
            }
        }
        Ok(_) => {
            Ok(false)
        }
        Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
            Ok(false)
        }
        Err(e) => Err(e),
    }
}


pub fn write_all_nb(buf: &mut BytesMut, stream: &mut Stream) -> io::Result<bool> {
    match stream.write(&buf) {
        Ok(n) => {
            buf.advance(n);
            Ok(buf.is_empty())
        }
        Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => Ok(false),
        Err(e) => Err(e.into()),
    }
}

pub fn write_all_nb_loop(buf: &mut BytesMut, stream: &mut Stream) -> io::Result<bool> {
    while !buf.is_empty() {
        match stream.write(&buf) {
            Ok(0) => {
                // Это может указывать на разрыв соединения
                debug!("write zero");
                return Err(io::Error::new(io::ErrorKind::WriteZero, "write zero"));
            }
            Ok(n) => {
                buf.advance(n);
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                return Ok(false); 
            }
            Err(e) => return Err(e),
        }
    }

    Ok(true) // всё записано
}