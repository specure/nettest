use anyhow::Result;
use log::{debug, info, trace};
use mio::{net::TcpStream, Interest, Poll, Token};
use openssl_sys::*;
use std::io::{self, Read, Write};
use std::net::SocketAddr;
use std::ptr;

#[derive(Debug)]
pub struct OpenSslSysStream {
    stream: TcpStream,
    ssl: *mut SSL,
    ctx: *mut SSL_CTX,
    read_bio: *mut BIO,
    write_bio: *mut BIO,
}

impl OpenSslSysStream {
    pub fn new(addr: SocketAddr) -> Result<Self> {
        unsafe {
            debug!("Initializing OpenSSL...");
            openssl_sys::init();

            debug!("Creating SSL context...");
            let ctx = SSL_CTX_new(TLS_client_method());
            if ctx.is_null() {
                return Err(anyhow::anyhow!("Failed to create SSL context"));
            }

            debug!("Configuring SSL context...");
            SSL_CTX_set_verify(ctx, SSL_VERIFY_NONE, None);
            SSL_CTX_set_mode(ctx, SSL_MODE_RELEASE_BUFFERS);
            SSL_CTX_set_options(ctx, SSL_OP_NO_COMPRESSION);
            
            // Устанавливаем TLS 1.3 шифры
            let cipher_list = b"TLS_AES_128_GCM_SHA256\0".as_ptr() as *mut i8;
            if SSL_CTX_set_ciphersuites(ctx, cipher_list) != 1 {
                SSL_CTX_free(ctx);
                return Err(anyhow::anyhow!("Failed to set TLS 1.3 ciphersuites"));
            }

            // Устанавливаем минимальную версию TLS 1.2
            if SSL_CTX_set_min_proto_version(ctx, TLS1_2_VERSION) != 1 {
                SSL_CTX_free(ctx);
                return Err(anyhow::anyhow!("Failed to set minimum TLS version"));
            }

            debug!("Creating SSL structure...");
            let ssl = SSL_new(ctx);
            if ssl.is_null() {
                SSL_CTX_free(ctx);
                return Err(anyhow::anyhow!("Failed to create SSL structure"));
            }

            debug!("Setting hostname...");
            let hostname = b"specure.com\0".as_ptr() as *mut i8;
            if SSL_set_tlsext_host_name(ssl, hostname) != 1 {
                SSL_free(ssl);
                SSL_CTX_free(ctx);
                return Err(anyhow::anyhow!("Failed to set hostname"));
            }

            debug!("Creating BIOs...");
            let read_bio = BIO_new(BIO_s_mem());
            let write_bio = BIO_new(BIO_s_mem());
            if read_bio.is_null() || write_bio.is_null() {
                SSL_free(ssl);
                SSL_CTX_free(ctx);
                return Err(anyhow::anyhow!("Failed to create BIO"));
            }

            debug!("Setting up SSL BIOs...");
            SSL_set_bio(ssl, read_bio, write_bio);

            debug!("Creating TCP connection...");
            let mut stream = TcpStream::connect(addr)?;
            stream.set_nodelay(true)?;

            debug!("Setting SSL to client mode...");
            SSL_set_connect_state(ssl);

            debug!("Starting SSL handshake...");
            let mut poll = Poll::new()?;
            let mut events = mio::Events::with_capacity(128);
            let token = Token(0);

            poll.registry().register(&mut stream, token, Interest::READABLE | Interest::WRITABLE)?;

            let mut handshake_completed = false;
            let mut handshake_attempts = 0;
            const MAX_HANDSHAKE_ATTEMPTS: u32 = 10;

            while !handshake_completed && handshake_attempts < MAX_HANDSHAKE_ATTEMPTS {
                handshake_attempts += 1;
                debug!("Handshake attempt {}...", handshake_attempts);

                // Проверяем, есть ли данные для отправки
                let mut temp_buf = vec![0u8; 8192];
                let n = BIO_read(write_bio, temp_buf.as_mut_ptr() as *mut _, temp_buf.len() as i32);
                if n > 0 {
                    debug!("Writing {} bytes to socket during handshake", n);
                    match stream.write(&temp_buf[..n as usize]) {
                        Ok(_) => {}
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                            debug!("Socket write would block, waiting for events...");
                            poll.poll(&mut events, None)?;
                            continue;
                        }
                        Err(e) => return Err(e.into()),
                    }
                }

                // Пробуем выполнить handshake
                let ret = SSL_connect(ssl);
                if ret == 1 {
                    debug!("SSL_connect returned success");
                    handshake_completed = true;
                    break;
                }

                let err = SSL_get_error(ssl, ret);
                debug!("SSL_connect returned error: {}", err);

                match err {
                    SSL_ERROR_WANT_READ => {
                        debug!("SSL wants read...");
                        match stream.read(&mut temp_buf) {
                            Ok(n) if n > 0 => {
                                debug!("Read {} bytes from socket", n);
                                if BIO_write(read_bio, temp_buf.as_ptr() as *const _, n as i32) != n as i32 {
                                    return Err(anyhow::anyhow!("Failed to write to read BIO"));
                                }
                            }
                            Ok(0) => {
                                debug!("Connection closed by peer during handshake");
                                return Err(anyhow::anyhow!("Connection closed by peer during handshake"));
                            }
                            Ok(_) => {
                                debug!("Unexpected read result");
                                return Err(anyhow::anyhow!("Unexpected read result"));
                            }
                            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                                debug!("Socket read would block, waiting for events...");
                                poll.poll(&mut events, None)?;
                                continue;
                            }
                            Err(e) => return Err(e.into()),
                        }
                    }
                    SSL_ERROR_WANT_WRITE => {
                        debug!("SSL wants write...");
                        continue; // Будет обработано в следующей итерации
                    }
                    SSL_ERROR_SYSCALL => {
                        debug!("SSL syscall error during handshake");
                        return Err(anyhow::anyhow!("SSL syscall error during handshake"));
                    }
                    SSL_ERROR_SSL => {
                        debug!("SSL protocol error during handshake");
                        return Err(anyhow::anyhow!("SSL protocol error during handshake"));
                    }
                    _ => return Err(anyhow::anyhow!("SSL handshake failed: {}", err)),
                }
            }

            debug!("Handshake completed");

            if !handshake_completed {
                return Err(anyhow::anyhow!("SSL handshake failed after {} attempts", handshake_attempts));
            }

            debug!("Verifying SSL state...");
            if SSL_is_init_finished(ssl) != 1 {
                return Err(anyhow::anyhow!("SSL not fully initialized after handshake"));
            }

            // Проверяем, что все данные записаны
            let mut temp_buf = vec![0u8; 8192];
            let n = BIO_read(write_bio, temp_buf.as_mut_ptr() as *mut _, temp_buf.len() as i32);
            if n > 0 {
                debug!("Writing final {} bytes to socket", n);
                stream.write(&temp_buf[..n as usize])?;
            }

            debug!("SSL connection fully established and verified");

            Ok(Self {
                stream,
                ssl,
                ctx,
                read_bio,
                write_bio,
            })
        }
    }

    pub fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        unsafe {
            debug!("Starting SSL read operation, buffer size: {}", buf.len());
            
            // Проверяем состояние SSL
            if SSL_is_init_finished(self.ssl) != 1 {
                debug!("SSL not initialized");
                return Err(io::Error::new(io::ErrorKind::Other, "SSL not initialized"));
            }

            // Сначала проверяем, есть ли данные в SSL буфере
            let pending = SSL_pending(self.ssl);
            debug!("SSL pending bytes: {}", pending);
            
            if pending > 0 {
                let to_read = std::cmp::min(pending as usize, buf.len());
                debug!("Reading {} bytes from SSL buffer", to_read);
                let n = SSL_read(self.ssl, buf.as_mut_ptr() as *mut _, to_read as i32);
                if n > 0 {
                    debug!("Successfully read {} bytes from SSL buffer", n);
                    return Ok(n as usize);
                } else {
                    let err = SSL_get_error(self.ssl, n);
                    debug!("SSL read error from buffer: {}", err);
                    match err {
                        SSL_ERROR_WANT_READ | SSL_ERROR_WANT_WRITE => {
                            return Err(io::Error::new(io::ErrorKind::WouldBlock, "SSL read would block"));
                        }
                        _ => return Err(io::Error::new(io::ErrorKind::Other, "SSL read error")),
                    }
                }
            }

            // Если в SSL буфере нет данных, пробуем прочитать из сокета
            let mut temp_buf = vec![0u8; 8192];
            match self.stream.read(&mut temp_buf) {
                Ok(n) if n > 0 => {
                    debug!("Read {} bytes from socket", n);
                    // Записываем в BIO для чтения
                    let written = BIO_write(self.read_bio, temp_buf.as_ptr() as *const _, n as i32);
                    if written != n as i32 {
                        debug!("Failed to write to read BIO: written {} != expected {}", written, n);
                        return Err(io::Error::new(io::ErrorKind::Other, "Failed to write to read BIO"));
                    }
                    debug!("Successfully wrote {} bytes to read BIO", written);

                    // Теперь пробуем прочитать из SSL
                    let to_read = std::cmp::min(buf.len(), 8192);
                    let n = SSL_read(self.ssl, buf.as_mut_ptr() as *mut _, to_read as i32);
                    if n > 0 {
                        debug!("Successfully read {} bytes from SSL after socket read", n);
                        Ok(n as usize)
                    } else {
                        let err = SSL_get_error(self.ssl, n);
                        debug!("SSL read error after socket read: {}", err);
                        match err {
                            SSL_ERROR_WANT_READ | SSL_ERROR_WANT_WRITE => {
                                Err(io::Error::new(io::ErrorKind::WouldBlock, "SSL read would block"))
                            }
                            _ => Err(io::Error::new(io::ErrorKind::Other, "SSL read error")),
                        }
                    }
                }
                Ok(0) => {
                    debug!("Connection closed by peer");
                    Ok(0)
                }
                Ok(_) => {
                    debug!("Unexpected read result");
                    Err(io::Error::new(io::ErrorKind::Other, "Unexpected read result"))
                    
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    debug!("Socket read would block, checking write BIO");
                     return Err(io::Error::new(io::ErrorKind::WouldBlock, "SSL read would block"))
                    // Если сокет блокируется, проверяем, есть ли данные для записи в BIO
                   
                }
                Err(e) => {
                    debug!("Socket read error: {:?}", e);
                    Err(e)
                }
            }
        }
    }

    pub fn flush(&mut self) -> io::Result<()> {
        self.stream.flush()
    }

    pub fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        unsafe {
            debug!("Starting SSL write operation, buffer size: {}", buf.len());
            
            // Проверяем состояние SSL
            if SSL_is_init_finished(self.ssl) != 1 {
                debug!("SSL not initialized");
                return Err(io::Error::new(io::ErrorKind::Other, "SSL not initialized"));
            }

            // Сначала проверяем, есть ли данные в BIO для записи, которые нужно отправить
            let mut temp_buf = vec![0u8; 8192];
            let mut bio_data_sent = 0;
            loop {
                let n = BIO_read(self.write_bio, temp_buf.as_mut_ptr() as *mut _, temp_buf.len() as i32);
                if n <= 0 {
                    break;
                }
                debug!("Flushing {} bytes from write BIO", n);
                match self.stream.write(&temp_buf[..n as usize]) {
                    Ok(written) => {
                        bio_data_sent += written;
                        if written < n as usize {
                            // Не все данные были отправлены, возвращаем WouldBlock
                            debug!("Could not send all BIO data, would block");
                            return Err(io::Error::new(io::ErrorKind::WouldBlock, "SSL write would block"));
                        }
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        debug!("Socket write would block while flushing BIO");
                        return Err(io::Error::new(io::ErrorKind::WouldBlock, "SSL write would block"));
                    }
                    Err(e) => {
                        debug!("Socket write error while flushing BIO: {:?}", e);
                        return Err(e);
                    }
                }
            }

            // Теперь пробуем записать новые данные в SSL
            debug!("Writing {} bytes to SSL", buf.len());
            let n = SSL_write(self.ssl, buf.as_ptr() as *const _, buf.len() as i32);
            if n > 0 {
                let bytes_written_to_ssl = n as usize;
                debug!("Successfully wrote {} bytes to SSL", bytes_written_to_ssl);
                
                // Пытаемся отправить данные из BIO
                let mut total_written = 0;
                loop {
                    let n = BIO_read(self.write_bio, temp_buf.as_mut_ptr() as *mut _, temp_buf.len() as i32);
                    if n <= 0 {
                        break;
                    }

                    debug!("Read {} bytes from write BIO after SSL_write", n);
                    let bytes_to_write = &temp_buf[..n as usize];
                    let mut written = 0;
                    while written < bytes_to_write.len() {
                        match self.stream.write(&bytes_to_write[written..]) {
                            Ok(0) => {
                                debug!("Failed to write to socket: write returned 0");
                                return Err(io::Error::new(io::ErrorKind::WriteZero, "Failed to write to socket"));
                            }
                            Ok(n) => {
                                debug!("Wrote {} bytes to socket", n);
                                written += n;
                            }
                            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                                debug!("Socket write would block after SSL_write");
                                return Err(io::Error::new(io::ErrorKind::WouldBlock, "SSL write would block"));
                            }
                            Err(e) => {
                                debug!("Socket write error: {:?}", e);
                                return Err(e);
                            }
                        }
                    }
                    total_written += written;
                }

                debug!("Total bytes written to socket: {}, returning bytes written to SSL: {}", 
                    total_written, bytes_written_to_ssl);
                Ok(bytes_written_to_ssl)
            } else {
                let err = SSL_get_error(self.ssl, n);
                debug!("SSL write error: {}", err);
                match err {
                    SSL_ERROR_WANT_READ => {
                        debug!("SSL wants read during write");
                        Err(io::Error::new(io::ErrorKind::WouldBlock, "SSL write would block"))
                    }
                    SSL_ERROR_WANT_WRITE => {
                        debug!("SSL wants write during write");
                        Err(io::Error::new(io::ErrorKind::WouldBlock, "SSL write would block"))
                    }
                    _ => {
                        debug!("SSL write error: {}", err);
                        Err(io::Error::new(io::ErrorKind::Other, format!("SSL write error: {}", err)))
                    }
                }
            }
        }
    }
    

    pub fn register(&mut self, poll: &Poll, token: Token, interests: Interest) -> io::Result<()> {
        unsafe {
            let mut actual_interests = Interest::READABLE | Interest::WRITABLE;
            
            // Проверяем состояние SSL
            if SSL_is_init_finished(self.ssl) == 1 {
                // Проверяем наличие данных в SSL буфере
                let pending = SSL_pending(self.ssl);
                if pending > 0 {
                    debug!("SSL has {} pending bytes, registering for READ", pending);
                    actual_interests = Interest::READABLE;
                }

                // Проверяем, есть ли данные для записи в BIO
                let mut temp_buf = vec![0u8; 8192];
                let n = BIO_read(self.write_bio, temp_buf.as_mut_ptr() as *mut _, temp_buf.len() as i32);
                if n > 0 {
                    debug!("Write BIO has {} bytes, registering for WRITE", n);
                    actual_interests = Interest::WRITABLE;
                }
            }

            // Если нет специфических интересов, используем переданные
            if actual_interests == (Interest::READABLE | Interest::WRITABLE) {
                debug!("No specific interests, using provided: {:?}", interests);
                actual_interests = interests;
            } else {
                debug!("Using calculated interests: {:?}", actual_interests);
            }
            
            poll.registry().register(&mut self.stream, token, actual_interests)
        }
    }

    pub fn reregister(&mut self, poll: &Poll, token: Token, interests: Interest) -> io::Result<()> {
        self.register(poll, token, interests)
    }
}

impl Drop for OpenSslSysStream {
    fn drop(&mut self) {
        unsafe {
            SSL_free(self.ssl);
            SSL_CTX_free(self.ctx);
            // BIO освобождается автоматически при освобождении SSL
        }
    }
} 