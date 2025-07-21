use anyhow::Result;
use log::debug;
use mio::{net::TcpStream, Interest, Poll, Token};
use openssl::ssl::ErrorCode;
use openssl::ssl::{Ssl, SslContext, SslMethod, SslMode, SslOptions, SslStream};
use std::io::{self, Write};

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
        ctx.set_mode(SslMode::RELEASE_BUFFERS);
        ctx.set_options(SslOptions::NO_COMPRESSION);
        ctx.set_ciphersuites("TLS_AES_128_GCM_SHA256")?;

        // Устанавливаем версии протокола
        ctx.set_max_proto_version(Some(openssl::ssl::SslVersion::TLS1_3))?;
        ctx.set_min_proto_version(Some(openssl::ssl::SslVersion::TLS1_2))?;

        let mut ssl = Ssl::new(&ctx.build())?;
        ssl.set_hostname(hostname)?;

        debug!("Creating SSL stream");
        let mut stream = SslStream::new(ssl, stream)?;
        debug!("SSL stream created");

        // Устанавливаем неблокирующий режим
        let tcp_stream = stream.get_mut();
        tcp_stream.set_nodelay(true)?;

        // Создаем Poll для ожидания событий
        let mut poll = Poll::new()?;
        let mut events = mio::Events::with_capacity(128);

        // Регистрируем сокет для чтения и записи
        poll.registry().reregister(
            stream.get_mut(),
            Token(0),
            Interest::READABLE | Interest::WRITABLE,
        )?;
        debug!("Registered stream");

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

    pub fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self.stream.ssl_read(buf) {
            Ok(n) => Ok(n),
            Err(ref e) => {
                if e.code() == ErrorCode::WANT_READ {
                    // debug!("SSL needs to READ more data ");
                    Err(io::Error::new(
                        io::ErrorKind::WouldBlock,
                        "SSL needs to read more data",
                    ))
                } else if e.code() == ErrorCode::WANT_WRITE {
                    debug!("SSL needs to WRITE  more data in READ !!!!!!!");
                    Err(io::Error::new(
                        io::ErrorKind::WouldBlock,
                        "SSL needs to write more data",
                    ))
                } else if let Some(io_error) = e.io_error() {
                    if io_error.kind() == io::ErrorKind::WouldBlock {
                        Err(io::Error::new(
                            io::ErrorKind::WouldBlock,
                            "SSL read would block",
                        ))
                    } else {
                        Err(io::Error::new(io::ErrorKind::Other, e.to_string()))
                    }
                } else {
                    Err(io::Error::new(io::ErrorKind::Other, e.to_string()))
                }
            }
        }
    }

    pub fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self.stream.write(buf) {
            Ok(n) => Ok(n),
            Err(ref e) => {
                if e.kind() == io::ErrorKind::WouldBlock {
                    Err(io::Error::new(
                        io::ErrorKind::WouldBlock,
                        "SSL write would block",
                    ))
                } else {
                    Err(io::Error::new(io::ErrorKind::Other, e.to_string()))
                }
            }
        }
    }

    pub fn flush(&mut self) -> io::Result<()> {
        self.stream.flush()
    }

    pub fn close(&mut self) -> Result<()> {
        self.stream.shutdown()?;
        Ok(())
    }

    pub fn register(&mut self, poll: &Poll, token: Token, interest: Interest) -> io::Result<()> {
        poll.registry()
            .register(self.stream.get_mut(), token, interest)
    }

    pub fn reregister(&mut self, poll: &Poll, token: Token, interest: Interest) -> io::Result<()> {
        poll.registry()
            .reregister(self.stream.get_mut(), token, interest)
    }
}
