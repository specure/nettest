use std::io::{self, Read, Write};
use bytes::{Buf, BytesMut};
use mio::net::TcpStream;
use log::debug;

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
pub fn read_until(stream: &mut TcpStream, buffer: &mut BytesMut, until: &str) -> io::Result<bool> {
    let mut temp_buf = vec![0u8; 1024];
    
    match stream.read(&mut temp_buf) {
        Ok(n) if n > 0 => {
            buffer.extend_from_slice(&temp_buf[..n]);
            let buffer_str = String::from_utf8_lossy(buffer);
            debug!("[read_until] Received data: {}", buffer_str);
            
            if buffer_str.contains(until) {
                debug!("[read_until] Found target string: {}", until);
                Ok(true)
            } else {
                Ok(false)
            }
        }
        Ok(_) => {
            debug!("[read_until] Read 0 bytes");
            Ok(false)
        }
        Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
            debug!("[read_until] WouldBlock");
            Ok(false)
        }
        Err(e) => Err(e),
    }
}

/// Writes data from the buffer to the stream until the buffer is empty
/// 
/// # Arguments
/// 
/// * `stream` - The TCP stream to write to
/// * `buffer` - The buffer containing data to write
/// 
/// # Returns
/// 
/// * `Ok(true)` if all data was written
/// * `Ok(false)` if more data needs to be written (WouldBlock)
/// * `Err` if an error occurred during writing
pub fn write_all(stream: &mut TcpStream, buffer: &mut BytesMut) -> io::Result<bool> {
    if buffer.is_empty() {
        return Ok(true);
    }

    match stream.write(buffer) {
        Ok(0) => {
            if stream.peer_addr().is_err() {
                return Err(io::Error::new(
                    io::ErrorKind::ConnectionAborted,
                    "Connection closed",
                ));
            }
            Ok(false)
        }
        Ok(n) => {
            *buffer = BytesMut::from(&buffer[n..]);
            debug!("[write_all] Wrote {} bytes, {} remaining", n, buffer.len());
            Ok(buffer.is_empty())
        }
        Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
            debug!("[write_all] WouldBlock");
            Ok(false)
        }
        Err(e) => Err(e),
    }
}



pub fn write_all_nb(buf: &mut BytesMut, stream: &mut TcpStream) -> io::Result<bool> {
    match stream.write(&buf) {
        Ok(n) => {
            buf.advance(n);
            Ok(buf.is_empty())
        }
        Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => Ok(false),
        Err(e) => Err(e.into()),
    }
}

pub fn write_all_nb_loop(buf: &mut BytesMut, stream: &mut TcpStream) -> io::Result<bool> {
    while !buf.is_empty() {
        match stream.write(&buf) {
            Ok(0) => {
                // Это может указывать на разрыв соединения
                return Err(io::Error::new(io::ErrorKind::WriteZero, "write zero"));
            }
            Ok(n) => {
                buf.advance(n);
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                return Ok(false); // не можем продолжать прямо сейчас
            }
            Err(e) => return Err(e),
        }
    }

    Ok(true) // всё записано
}