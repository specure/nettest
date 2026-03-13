use anyhow::{Error, Result};
use log::{debug, info, trace};
use mio::{net::TcpStream, Interest, Poll, Token};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::{ServerConfig, ServerConnection};
use std::fs;
use std::io::{self, BufReader, Read, Write};
use std::path::Path;
use std::sync::Arc;

#[derive(Debug)]
pub struct RustlsServerStream {
    pub conn: ServerConnection,
    pub stream: TcpStream,
    pub finished: bool,
    pub temp_buf: Vec<u8>,
}

impl RustlsServerStream {
    pub fn new(stream: TcpStream, cert_path: String, key_path: String) -> Result<Self> {
        if let Err(_) = stream.set_nodelay(true) {
            std::thread::sleep(std::time::Duration::from_millis(1000));
            if let Err(e) = stream.set_nodelay(true) {
                info!("Failed to set TCP_NODELAY: {}", e);
            }
        }

        let certs = load_certs(Path::new(&cert_path))?;
        let key = load_private_key(Path::new(&key_path))?;

        let mut config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .map_err(|e| Error::msg(format!("Failed to create server config: {}", e)))?;

            config.alpn_protocols = vec![b"http/1.1".to_vec()];


        let conn = ServerConnection::new(Arc::new(config))?;

        info!("RustlsServerStream created");

        // conn.set_buffer_limit(Some(1024 * 1024 * 10));

        Ok(Self {
            conn,
            stream,
            finished: true,
            temp_buf: vec![],
        })
    }


    pub fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        
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
                // Force send data to clear buffer
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
                trace!("Read  {} bytes from TLS", n);
            }
        }

        trace!("Temp buf len: {}", self.temp_buf.len());

        // Process any new TLS messages
        // debug!("Processing new packets");
        let io_state = match self.conn.process_new_packets() {
            Ok(state) => {
                debug!(
                    "Processed new packets {:?}",
                    state.plaintext_bytes_to_read()
                );
                state
            }
            Err(e) => {
                trace!("Error processing new packets: {:?}", e);
                return Err(io::Error::new(io::ErrorKind::Other, e));
            }
        };

        

        let mut chunk = [0u8; 8192 * 10];

        loop {
            match self.conn.reader().read(&mut chunk) {
                Ok(0) => {
                    trace!("EOF");
                    break;
                }
                Ok(n) => {
                    self.temp_buf.extend_from_slice(&chunk[..n]);
                    trace!("Extending temp buf by {} bytes", n);
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                    trace!("WouldBlock nothing to read from conn");
                    break;
                }
                Err(e) => return Err(e),
            }
        }

        let to_copy = buf.len().min(self.temp_buf.len());

        buf[..to_copy].copy_from_slice(&self.temp_buf[..to_copy]);

        trace!(
            "To copy: {} from temp buf len: {}",
            to_copy,
            self.temp_buf.len()
        );
        if to_copy > 0 {
            self.temp_buf.drain(..to_copy);
            trace!("Temp buf len after drain : {}", self.temp_buf.len());
            return Ok(to_copy);
        }

        trace!("No data to copy");
        // If rustls needs more network data to complete a TLS record, read more immediately.
        // This is essential for edge-triggered epoll (MIO): if data is already in the socket
        // buffer, EPOLLET won't fire a new event unless we drain it here.
        if self.conn.wants_read() {
            trace!("Wants read");
            return self.read(buf);
        }

        // Force send data to clear buffer
        if self.conn.wants_write() {
            trace!("Wants write");
            self.conn.write_tls(&mut self.stream)?;
        }

        // Check if peer has closed
        if io_state.peer_has_closed() {
            trace!("Peer has closed the connection");
            return Ok(0);
        }

        // TLS processed an internal record (session ticket / key update) with no app data.
        // Signal the caller to wait for the next readable event.
        Err(io::Error::new(io::ErrorKind::WouldBlock, "no plaintext data yet"))
    }

    pub fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut total_written = 0;

        // Check if there's data in buffer to send
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
                    info!("TLS buffer flush error: {:?}", e);
                    return Err(e);
                }
            }
        }

        if !self.finished {
            total_written += 1;
            self.finished = true;
        }

        // Now try to write new data
        while total_written < buf.len() {
            match self.conn.writer().write(&buf[total_written..]) {
                Ok(n) => {
                    total_written += n;

                    // Try to send data to network
                    while self.conn.wants_write() {
                        match self.conn.write_tls(&mut self.stream) {
                            Ok(_) => {
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

        // Send close_notify
        if self.conn.wants_write() {
            match self.conn.write_tls(&mut self.stream) {
                Ok(_) => debug!("Sent TLS close_notify"),
                Err(e) => debug!("Error sending TLS close_notify: {:?}", e),
            }
        }

        // Close TCP connection
        self.stream.shutdown(std::net::Shutdown::Both)
    }
}

pub fn load_certs(cert_path: &Path) -> Result<Vec<CertificateDer<'static>>, Error> {
    let cert_path = cert_path.to_str().unwrap();
    debug!("Loading certificates from {}", cert_path);
    let certfile = fs::read(cert_path)?;
    trace!("Read {} bytes from certificate file", certfile.len());
    let mut reader = BufReader::new(certfile.as_slice());
    let certs = rustls_pemfile::certs(&mut reader).collect::<Result<Vec<_>, _>>()?;
    debug!("Successfully parsed {} certificates", certs.len());
    Ok(certs)
}

pub fn load_private_key(key_path: &Path) -> Result<PrivateKeyDer<'static>, Error> {
    let key_path = key_path.to_str().unwrap();
    debug!("Loading private key from {}", key_path);
    let keyfile = fs::read(key_path)?;
    trace!("Read {} bytes from key file", keyfile.len());
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
