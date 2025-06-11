use crate::rustls::RustlsStream;
use anyhow::Result;
use log::{debug, trace};
use mio::{net::TcpStream, Interest, Poll, Token};
use openssl::ssl::{Ssl, SslContext, SslMethod, SslStream};
use std::io::{self, Read, Write};
use std::net::SocketAddr;
use std::path::Path;

#[derive(Debug)]
pub enum Stream {
    Tcp(TcpStream),
    Rustls(RustlsStream),
    OpenSsl(OpenSslStream),
}

#[derive(Debug)]
pub struct OpenSslStream {
    pub stream: SslStream<TcpStream>,
}

impl OpenSslStream {
    pub fn new(stream: TcpStream, hostname: &str) -> Result<Self> {
        let mut ctx = SslContext::builder(SslMethod::tls_client())?;
        ctx.set_verify(openssl::ssl::SslVerifyMode::NONE);
        let ssl = Ssl::new(&ctx.build())?;
        let stream = SslStream::new(ssl, stream)?;
        Ok(Self { stream })
    }
}

impl Stream {
    pub fn new_tcp(addr: SocketAddr) -> Result<Self> {
        let stream = TcpStream::connect(addr)?;
        stream.set_nodelay(true)?;
        Ok(Self::Tcp(stream))
    }

    pub fn new_rustls(addr: SocketAddr, cert_path: Option<&Path>, key_path: Option<&Path>) -> Result<Self> {
        let stream = TcpStream::connect(addr)?;
        stream.set_nodelay(true)?;
        let stream = RustlsStream::new(addr, cert_path, key_path)?;
        Ok(Self::Rustls(stream))
    }

    pub fn new_openssl(addr: SocketAddr) -> Result<Self> {
        let stream = TcpStream::connect(addr)?;
        stream.set_nodelay(true)?;
        let stream = OpenSslStream::new(stream, "localhost")?;
        Ok(Self::OpenSsl(stream))
    }

    pub fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Stream::Tcp(stream) => stream.read(buf),
            Stream::Rustls(stream) => stream.read(buf),
            Stream::OpenSsl(stream) => stream.stream.read(buf),
        }
    }

    pub fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Stream::Tcp(stream) => stream.write(buf),
            Stream::Rustls(stream) => stream.write(buf),
            Stream::OpenSsl(stream) => stream.stream.write(buf),
        }
    }

    pub fn register(&mut self, poll: &Poll, token: Token, interest: Interest) -> Result<()> {
        match self {
            Stream::Tcp(stream) => {
                poll.registry().register(stream, token, interest)?;
            }
            Stream::Rustls(stream) => {
                poll.registry().register(&mut stream.stream, token, interest)?;
            }
            Stream::OpenSsl(stream) => {
                let tcp_stream = stream.stream.get_mut();
                poll.registry().register(tcp_stream, token, interest)?;
            }
        }
        Ok(())
    }

    pub fn reregister(&mut self, poll: &Poll, token: Token, interest: Interest) -> Result<()> {
        match self {
            Stream::Tcp(stream) => {
                poll.registry().reregister(stream, token, interest)?;
            }
            Stream::Rustls(stream) => {
                poll.registry().reregister(&mut stream.stream, token, interest)?;
            }
            Stream::OpenSsl(stream) => {
                let tcp_stream = stream.stream.get_mut();
                poll.registry().reregister(tcp_stream, token, interest)?;
            }
        }
        Ok(())
    }
} 