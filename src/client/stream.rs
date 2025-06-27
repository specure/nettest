use anyhow::{Ok, Result};
use mio::{net::TcpStream, Interest, Poll, Token};
use std::io::{self, Read, Write};
use std::net::SocketAddr;
use std::path::Path;

use crate::client::openssl::OpenSslStream;
use crate::client::rustls::RustlsStream;
use crate::client::websocket::WebSocketClient;
use crate::client::websocket_tls_openssl::WebSocketTlsClient;
use crate::client::RMBT_UPGRADE_REQUEST;

#[derive(Debug)]
pub enum Stream {
    Tcp(TcpStream),
    WebSocket(WebSocketClient),
    OpenSsl(OpenSslStream),
    Rustls(RustlsStream),
    WebSocketTls(WebSocketTlsClient),
}

impl Stream {
    pub fn new_tcp(addr: SocketAddr) -> Result<Self> {
        let stream = TcpStream::connect(addr)?;
        stream.set_nodelay(true)?;
        Ok(Self::Tcp(stream))
    }

    pub fn return_type(&self) -> &str {
        match self {
            Stream::Tcp(_) => "Tcp",
            Stream::OpenSsl(_) => "OpenSsl",
            Stream::WebSocket(_) => "WebSocket",
            Stream::Rustls(_) => "Rustls",
            Stream::WebSocketTls(_) => "WebSocketTls",
        }
    }

    pub fn new_websocket(addr: SocketAddr) -> Result<Self> {
        let ws_client = WebSocketClient::new(addr)?;
        Ok(Self::WebSocket(ws_client))
    }

    pub fn new_rustls(addr: SocketAddr, cert_path: Option<&Path>, key_path: Option<&Path>) -> Result<Self> {
        let stream = RustlsStream::new(addr, cert_path, key_path)?;
        Ok(Self::Rustls(stream))
    }

    pub fn close(&mut self) -> Result<()> {
        match self {
            Stream::Tcp(_) => Ok(()),
            Stream::OpenSsl(stream) => stream.close(),
            Stream::WebSocket(stream) => stream.close(),
            Stream::Rustls(_) => Ok(()),
            Stream::WebSocketTls(stream) => stream.close(),
        }
    }

    pub fn get_greeting(&mut self) -> Vec<u8> {
        match self {
            Stream::Tcp(_) => RMBT_UPGRADE_REQUEST.as_bytes().to_vec(),
            Stream::OpenSsl(_) => RMBT_UPGRADE_REQUEST.as_bytes().to_vec(),
            Stream::WebSocket(_) => RMBT_UPGRADE_REQUEST.as_bytes().to_vec(),
            Stream::Rustls(_) => RMBT_UPGRADE_REQUEST.as_bytes().to_vec(),
            Stream::WebSocketTls(_) => RMBT_UPGRADE_REQUEST.as_bytes().to_vec(),
        }
    }

    pub fn new_openssl(addr: SocketAddr) -> Result<Self> {
        let stream1 = TcpStream::connect(addr)?;
        stream1.set_nodelay(true)?;
        let stream = OpenSslStream::new(stream1, "localhost")?;
        Ok(Self::OpenSsl(stream))
    }

    pub fn new_websocket_tls(addr: SocketAddr) -> Result<Self> {
        let stream1 = TcpStream::connect(addr)?;
        stream1.set_nodelay(true)?;
        let stream = WebSocketTlsClient::new(addr,stream1, "localhost")?;
        Ok(Self::WebSocketTls(stream))
    }

    pub fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Stream::Tcp(stream) => stream.read(buf),
            Stream::OpenSsl(stream) => stream.read(buf),
            Stream::WebSocket(stream) => stream.read(buf),
            Stream::Rustls(stream) => stream.read(buf),
            Stream::WebSocketTls(stream) => stream.read(buf),
        }
    }

    pub fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Stream::Tcp(stream) => stream.write(buf),
            Stream::OpenSsl(stream) => stream.write(buf),
            Stream::WebSocket(stream) => stream.write(buf),
            Stream::Rustls(stream) => stream.write(buf),
            Stream::WebSocketTls(stream) => stream.write(buf),
        }
    }

    pub fn register(&mut self, poll: &Poll, token: Token, interest: Interest) -> Result<()> {
        match self {
            Stream::Tcp(stream) => {
                poll.registry().register(stream, token, interest)?;
            }
            Stream::OpenSsl(stream) => {
                stream.register(poll, token, interest)?;
            }
            Stream::WebSocket(stream) => {
               stream.register(poll, token, interest)?;
            }
            Stream::Rustls(stream) => {
                stream.register(poll, token, interest)?;
            }
            Stream::WebSocketTls(stream) => {
                stream.register(poll, token, interest)?;
            }
        }
        Ok(())
    }

    pub fn flush(&mut self) -> io::Result<()> {
        match self {
            Stream::Tcp(stream) => stream.flush(),
            Stream::OpenSsl(stream) => stream.flush(),
            Stream::WebSocket(stream) => stream.flush(),
            Stream::Rustls(stream) => stream.flush(),
            Stream::WebSocketTls(stream) => stream.flush(),
        }
    }

    pub fn reregister(&mut self, poll: &Poll, token: Token, interest: Interest) -> Result<()> {
        match self {
            Stream::Tcp(stream) => {
                poll.registry().reregister(stream, token, interest)?;
            }
            Stream::OpenSsl(stream) => {
                stream.reregister(poll, token, interest)?;
            }
            Stream::WebSocket(stream) => {
                stream.reregister(poll, token, interest)?;
            }
            Stream::Rustls(stream) => {
                stream.reregister(poll, token, interest)?;
            }
            Stream::WebSocketTls(stream) => {
                stream.reregister(poll, token, interest)?;
            }
        }
        Ok(())
    }
}
