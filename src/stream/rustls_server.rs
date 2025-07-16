use anyhow::{Error, Result};
use log::{debug, info, trace};
use mio::{net::TcpStream, Interest, Poll, Token};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::{ServerConfig, ServerConnection};
use std::fs;
use std::io::{self, BufReader, Read, Write};
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

#[derive(Debug)]
pub struct RustlsServerStream {
    pub conn: ServerConnection,
    pub stream: TcpStream,
    handshake_done: bool,
    pub finished: bool,
    pub temp_buf: Vec<u8>,
}

impl RustlsServerStream {
    pub fn new(
        stream: TcpStream,
        cert_path: String,
        key_path: String,
    ) -> Result<Self> {
        stream.set_nodelay(true)?;

        let config = if let (cert_path, key_path) = (cert_path, key_path) {
            let certs = load_certs(Path::new(&cert_path))?;
            let key = load_private_key(Path::new(&key_path))?;

            let config = ServerConfig::builder()
                .with_no_client_auth()
                .with_single_cert(certs, key)
                .map_err(|e| Error::msg(format!("Failed to create server config: {}", e)))?;
            config
        } else {
            return Err(Error::msg(
                "Certificate and key paths are required for server",
            ));
        };

        let mut conn = ServerConnection::new(Arc::new(config))?;

        // conn.set_buffer_limit(Some(1024 * 1024 * 10));

        Ok(Self {
            conn,
            stream,
            handshake_done: false,
            finished: true,
            temp_buf: vec![],
        })
    }

    pub fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        // trace!("Reading from RustlsStream");
        if self.temp_buf.len() > buf.len() {

            let to_copy = buf.len().min(self.temp_buf.len());
            buf[..to_copy].copy_from_slice(&self.temp_buf[..to_copy]);
            self.temp_buf.drain(..to_copy);
            debug!("Left {} bytes in temp_buf", self.temp_buf.len());
            return Ok(to_copy);
        } 



        // Read TLS data
        match self.conn.read_tls(&mut self.stream) {
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                // trace!("TLS read would block");
                // Принудительно отправляем данные для очистки буфера
                if self.conn.wants_write() {
                    self.conn.write_tls(&mut self.stream)?;
                }
                debug!("WouldBlock 1");
                if self.temp_buf.len() > 0 {

                    let to_copy = buf.len().min(self.temp_buf.len());
                    buf[..to_copy].copy_from_slice(&self.temp_buf[..to_copy]);
                    self.temp_buf.drain(..to_copy);
                    debug!("Left {} bytes in temp_buf", self.temp_buf.len());
                    return Ok(to_copy);
                }  
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "TLS read would block",
                ));
            }
            Err(e) => {
                debug!("TLS read error: {:?}", e);
                return Err(e);
            }
            Ok(0) => {
                // trace!("TLS connection closed");
                return Ok(0);
            }
            Ok(n) => {
            }
        }

        // Process any new TLS messages
        // debug!("Processing new packets");
        let io_state = match self.conn.process_new_packets() {
            Ok(state) => {
                // debug!("Processed new packets {:?}", state.plaintext_bytes_to_read());
                state
            }
            Err(e) => {
                trace!("Error processing new packets: {:?}", e);
                return Err(io::Error::new(io::ErrorKind::Other, e));
            }
        };

    

        // If we need to write handshake data, do it
        if !self.handshake_done {
            while self.conn.wants_write() {
                self.handshake_done = true;
                trace!("Writing handshake data");
                self.conn.write_tls(&mut self.stream)?;
            }
            self.handshake_done = true;
        }

        // Read any new plaintext
        // let mut total_read = 0;
        // while io_state.plaintext_bytes_to_read() > 0  {
        //     match self.conn.reader().read(&mut buf[total_read..]) {
        //         Ok(0) => break,
        //         Ok(n) => {
        //             debug!("Read {} bytes of new data", n);
        //             total_read += n;
        //         }
        //         Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
        //         Err(e) => return Err(e),
        //     }
        // }

        // let mut temp_buf = Vec::new();
        let mut chunk = [0u8; 8192];
            

        loop {
            match self.conn.reader().read(&mut chunk) {
                Ok(0) => {
                    break;
                },
                Ok(n) => {
                    self.temp_buf.extend_from_slice(&chunk[..n]);
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                    break;
                },
                Err(e) => return Err(e),
            }
        }

        let to_copy = buf.len().min(self.temp_buf.len());
        buf[..to_copy].copy_from_slice(&self.temp_buf[..to_copy]);




        if to_copy > 0 {
            self.temp_buf.drain(..to_copy);
            debug!("Temp buf len: {}", self.temp_buf.len());
            return Ok(to_copy);
        }

        // If we need to read more data, try again
        if self.conn.wants_read() {
            return self.read(buf);
        }

        // Принудительно отправляем данные для очистки буфера
        if self.conn.wants_write() {
            self.conn.write_tls(&mut self.stream)?;
        }

        // Check if peer has closed
        if io_state.peer_has_closed() {
            trace!("Peer has closed the connection");
            return Ok(0);
        }

        Ok(0)
    }

    pub fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut total_written = 0;

        // Проверяем, есть ли данные в буфере для отправки
        while self.conn.wants_write() {
            match self.conn.write_tls(&mut self.stream) {
                Ok(_) => {}
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    trace!("TLS buffer flush would block");
                    return Err(io::Error::new(
                        io::ErrorKind::WouldBlock,
                        "TLS write would block",
                    ));
                }
                Err(e) => {
                    debug!("TLS buffer flush error: {:?}", e);
                    return Err(e);
                }
            }
        }

        if !self.finished {
            total_written += 1;
            self.finished = true;
        }


        // Теперь пробуем записать новые данные
        while total_written < buf.len() {
            match self.conn.writer().write(&buf[total_written..]) {
                Ok(n) => {
                    total_written += n;

                    // Пытаемся отправить данные в сеть
                    while self.conn.wants_write() {
                        match self.conn.write_tls(&mut self.stream) {
                            Ok(s) => {
                                continue;
                            }
                            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                                self.finished = false;
                                return Ok(total_written - 1);
                            }
                            Err(e) => {
                                debug!("Network write error: {:?}", e);
                                return Err(e);
                            }
                        }
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    debug!("TLS buffer write would block");
                    return Err(io::Error::new(
                        io::ErrorKind::WouldBlock,
                        "TLS write would block",
                    ));
                }
                Err(e) => {
                    debug!("TLS buffer write error: {:?}", e);
                    return Err(e);
                }
            }
        }


        Ok(total_written)
    }

    pub fn flush(&mut self) -> io::Result<()> {
        self.stream.flush()
    }

    pub fn register(&mut self, poll: &Poll, token: Token, interests: Interest) -> io::Result<()> {
        poll.registry().register(&mut self.stream, token, interests)
    }

    pub fn reregister(&mut self, poll: &Poll, token: Token, interests: Interest) -> io::Result<()> {
        poll.registry()
            .reregister(&mut self.stream, token, interests)
    }

    pub fn close(&mut self) -> io::Result<()> {
        debug!("Closing TLS connection");

        // Отправляем close_notify
        if self.conn.wants_write() {
            match self.conn.write_tls(&mut self.stream) {
                Ok(_) => debug!("Sent TLS close_notify"),
                Err(e) => debug!("Error sending TLS close_notify: {:?}", e),
            }
        }

        // Закрываем TCP соединение
        self.stream.shutdown(std::net::Shutdown::Both)
    }
}

pub fn load_certs(cert_path: &Path) -> Result<Vec<CertificateDer<'static>>, Error> {
    let cert_path = cert_path.to_str().unwrap();
    debug!("Loading certificates from {}", cert_path);
    let certfile = fs::read(cert_path)?;
    debug!("Read {} bytes from certificate file", certfile.len());
    let mut reader = BufReader::new(certfile.as_slice());
    let certs = rustls_pemfile::certs(&mut reader).collect::<Result<Vec<_>, _>>()?;
    debug!("Successfully parsed {} certificates", certs.len());
    Ok(certs)
}

pub fn load_private_key(key_path: &Path) -> Result<PrivateKeyDer<'static>, Error> {
    let key_path = key_path.to_str().unwrap();
    debug!("Loading private key from {}", key_path);
    let keyfile = fs::read(key_path)?;
    debug!("Read {} bytes from key file", keyfile.len());
    let mut reader = BufReader::new(keyfile.as_slice());

    // Try to read any private key format
    if let Some(key) = rustls_pemfile::private_key(&mut reader)? {
        debug!("Successfully loaded private key: {:?}", key);
        Ok(key)
    } else {
        debug!("No private keys found in key file");
        Err(Error::msg("No private keys found in key file"))
    }
}


impl Read for RustlsServerStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.read(buf)
    }
}

impl Write for RustlsServerStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.flush()
    }
}