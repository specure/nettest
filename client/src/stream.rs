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
        debug!("Creating SSL context");
        let mut ctx = SslContext::builder(SslMethod::tls_client())?;
        debug!("Setting verify mode to NONE");
        ctx.set_verify(openssl::ssl::SslVerifyMode::NONE);
        let mut ssl = Ssl::new(&ctx.build())?;
        ssl.set_hostname(hostname)?;
        debug!("Creating SSL stream");
        let mut stream = SslStream::new(ssl, stream)?;
        debug!("SSL stream created");

        // Устанавливаем неблокирующий режим
        stream.get_mut().set_nodelay(true)?;

        // Создаем Poll для ожидания событий
        let mut poll = Poll::new()?;
        let mut events = mio::Events::with_capacity(128);

        // Регистрируем сокет для чтения и записи
        poll.registry().register(
            stream.get_mut(),
            Token(0),
            Interest::READABLE | Interest::WRITABLE,
        )?;

        // Ждем, пока TCP соединение будет установлено
        loop {
            poll.poll(&mut events, None)?;
            let mut connection_ready = false;
            
            for event in events.iter() {
                if event.is_writable() {
                    // Проверяем, что соединение действительно установлено
                    if let Err(e) = stream.get_ref().peer_addr() {
                        if e.kind() == io::ErrorKind::NotConnected {
                            debug!("TCP connection not ready yet, waiting...");
                            continue;
                        }
                    }
                    connection_ready = true;
                    break;
                }
            }
            
            if connection_ready {
                break;
            }
        }

        // Теперь можно начинать TLS handshake
        loop {
            match stream.connect() {
                Ok(_) => {
                    debug!("Handshake completed");
                    break;
                }
                Err(e) => {
                    // Проверяем на вложенную ошибку WouldBlock
                    if let Some(io_error) = e.io_error() {
                        if io_error.kind() == io::ErrorKind::WouldBlock {
                            debug!("Socket not ready, waiting...");
                            poll.poll(&mut events, None)?;
                            continue;
                        }
                    }
                    debug!("Error during handshake: {:?}", e);
                    return Err(e.into());
                }
            }
        }

        Ok(Self { stream })
    }
}

impl Stream {
    pub fn new_tcp(addr: SocketAddr) -> Result<Self> {
        let stream = TcpStream::connect(addr)?;
        stream.set_nodelay(true)?;
        Ok(Self::Tcp(stream))
    }

    pub fn new_rustls(
        addr: SocketAddr,
        cert_path: Option<&Path>,
        key_path: Option<&Path>,
    ) -> Result<Self> {
        let stream = TcpStream::connect(addr)?;
        stream.set_nodelay(true)?;
        let stream = RustlsStream::new(addr, cert_path, key_path)?;
        Ok(Self::Rustls(stream))
    }

    pub fn new_openssl(addr: SocketAddr) -> Result<Self> {
        let stream = TcpStream::connect(addr)?;
        stream.set_nodelay(true)?;
        let stream = OpenSslStream::new(stream, "specure.com")?;
        Ok(Self::OpenSsl(stream))
    }

    pub fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Stream::Tcp(stream) => stream.read(buf),
            Stream::Rustls(stream) => stream.read(buf),
            Stream::OpenSsl(stream) => match stream.stream.read(buf) {
                Ok(n) => Ok(n),
                Err(e) => {
                    if e.kind() == io::ErrorKind::WouldBlock {
                        Err(io::Error::new(
                            io::ErrorKind::WouldBlock,
                            "Resource temporarily unavailable",
                        ))
                    } else {
                        Err(e)
                    }
                }
            },
        }
    }

    pub fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Stream::Tcp(stream) => stream.write(buf),
            Stream::Rustls(stream) => stream.write(buf),
            Stream::OpenSsl(stream) => match stream.stream.write(buf) {
                Ok(n) => Ok(n),
                Err(e) => {
                    if e.kind() == io::ErrorKind::WouldBlock {
                        Err(io::Error::new(
                            io::ErrorKind::WouldBlock,
                            "Resource temporarily unavailable",
                        ))
                    } else {
                        Err(e)
                    }
                }
            },
        }
    }

    pub fn register(&mut self, poll: &Poll, token: Token, interest: Interest) -> Result<()> {
        match self {
            Stream::Tcp(stream) => {
                poll.registry().register(stream, token, interest)?;
            }
            Stream::Rustls(stream) => {
                poll.registry()
                    .register(&mut stream.stream, token, interest)?;
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
                poll.registry()
                    .reregister(&mut stream.stream, token, interest)?;
            }
            Stream::OpenSsl(stream) => {
                let tcp_stream = stream.stream.get_mut();
                poll.registry().reregister(tcp_stream, token, interest)?;
            }
        }
        Ok(())
    }
}
