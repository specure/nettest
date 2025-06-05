use anyhow::Result;
use mio::{net::TcpStream, Interest, Poll, Token};
use rustls::{ClientConfig, ClientConnection};
use std::io::{self, Read, Write};
use std::net::SocketAddr;
use std::sync::Arc;

pub enum Stream {
    Tcp(TcpStream),
    Tls(ClientConnection, TcpStream),
}

impl Stream {
    pub fn new_tcp(addr: SocketAddr) -> Result<Self> {
        let stream = TcpStream::connect(addr)?;
        Ok(Stream::Tcp(stream))
    }

    pub fn new_tls(addr: SocketAddr, config: Arc<ClientConfig>) -> Result<Self> {
        let stream = TcpStream::connect(addr)?;
        let server_name = "localhost".try_into()?;
        let conn = ClientConnection::new(config, server_name)?;
        Ok(Stream::Tls(conn, stream))
    }

    pub fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Stream::Tcp(stream) => stream.read(buf),
            Stream::Tls(conn, stream) => {
                while conn.wants_read() {
                    match conn.read_tls(stream) {
                        Ok(0) => return Ok(0),
                        Ok(_) => {
                            if let Err(e) = conn.process_new_packets() {
                                return Err(io::Error::new(io::ErrorKind::Other, e));
                            }
                        }
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                        Err(e) => return Err(e),
                    }
                }
                conn.reader().read(buf)
            }
        }
    }

    pub fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Stream::Tcp(stream) => stream.write(buf),
            Stream::Tls(conn, stream) => {
                let mut total_written = 0;
                while total_written < buf.len() {
                    match conn.writer().write(&buf[total_written..]) {
                        Ok(n) => {
                            total_written += n;
                            while conn.wants_write() {
                                match conn.write_tls(stream) {
                                    Ok(_) => {}
                                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                                    Err(e) => return Err(e),
                                }
                            }
                        }
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                        Err(e) => return Err(e),
                    }
                }
                Ok(total_written)
            }
        }
    }

    pub fn register(&self, poll: &Poll, token: Token, interests: Interest) -> io::Result<()> {
        match self {
            Stream::Tcp(stream) => poll.registry().register(stream, token, interests),
            Stream::Tls(_, stream) => poll.registry().register(stream, token, interests),
        }
    }

    pub fn reregister(&self, poll: &Poll, token: Token, interests: Interest) -> io::Result<()> {
        match self {
            Stream::Tcp(stream) => poll.registry().reregister(stream, token, interests),
            Stream::Tls(_, stream) => poll.registry().reregister(stream, token, interests),
        }
    }
} 