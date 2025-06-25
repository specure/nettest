use anyhow::{Error, Result};
use core::error;
use log::{debug, error, info, trace};
use mio::{net::TcpStream, Events, Interest, Poll, Token};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName};
use rustls::{ClientConfig, ClientConnection, RootCertStore};
use std::fs;
use std::io::{self, BufReader, Read, Write};
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use rustls::CipherSuite::TLS13_AES_128_GCM_SHA256;

use crate::utils::MAX_CHUNK_SIZE;

#[derive(Debug)]
pub struct RustlsStream {
    pub conn: ClientConnection,
    pub stream: TcpStream,
    handshake_done: bool,
}

impl RustlsStream {
    pub fn new(
        addr: SocketAddr,
        cert_path: Option<&Path>,
        key_path: Option<&Path>,
    ) -> Result<Self> {
        let mut stream = TcpStream::connect(addr)?;
        stream.set_nodelay(true)?;

        let config = if let (Some(cert_path), Some(key_path)) = (cert_path, key_path) {
            let mut root_store = RootCertStore::empty();
            let certs = load_certs(cert_path)?;

            for cert in &certs {
                root_store.add(cert.clone())?;
            }

            let key = load_private_key(key_path)?;

            let config = ClientConfig::builder()
                .with_root_certificates(root_store)
                .with_client_auth_cert(certs, key)?;
            config
        } else {
            let config = ClientConfig::builder()
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(danger::NoCertificateVerification))
                .with_no_client_auth();
            config
        };

        // Используем IP-адрес как домен для локального соединения
        let server_name = if addr.ip().is_loopback() {
            ServerName::DnsName("localhost".try_into()?)
        } else {
            ServerName::DnsName("dev.measurementservers.net".try_into()?)
        };

        let mut conn = ClientConnection::new(Arc::new(config), server_name)?;

        // conn.set_buffer_limit(Some(1024 * 1024 * 10));

        Ok(Self {
            conn,
            stream,
            handshake_done: false,
        })
    }

    pub fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        // trace!("Reading from RustlsStream");

        // Read TLS data
        match self.conn.read_tls(&mut self.stream) {
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                debug!("TLS read would block");
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
                debug!("TLS connection closed");
                return Ok(0);
            }
            Ok(n) => {
                debug!("Read {} bytes from TLS stream", n);
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
                debug!("Writing handshake data");
                self.conn.write_tls(&mut self.stream)?;
            }
            self.handshake_done = true;
        }

        // Read any new plaintext
        if io_state.plaintext_bytes_to_read() > 0 {
            let n = self.conn.reader().read(buf)?;
            trace!("Read {} bytes of new data", n);
            return Ok(n);
        }

        // If we need to read more data, try again
        if self.conn.wants_read() {
            debug!("Need to read more data");
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
                    debug!("TLS buffer flush would block");
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

        // Теперь пробуем записать новые данные
        while total_written < buf.len() {
            match self.conn.writer().write(&buf[total_written..]) {
                Ok(n) => {
                    total_written += n;

                    // Пытаемся отправить данные в сеть
                    while self.conn.wants_write() {
                        match self.conn.write_tls(&mut self.stream) {
                            Ok(_) => {
                                continue;
                            }
                            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                                debug!("Network write would block after {} bytes", total_written);
                                break;
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

    pub fn perform_handshake(&mut self) -> Result<()> {
        info!("TLS handshake completed successfully");
        Ok(())
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

fn load_certs(cert_path: &Path) -> Result<Vec<CertificateDer<'static>>, Error> {
    let cert_path = cert_path.to_str().unwrap();
    debug!("Loading certificates from {}", cert_path);
    let certfile = fs::read(cert_path)?;
    debug!("Read {} bytes from certificate file", certfile.len());
    let mut reader = BufReader::new(certfile.as_slice());
    let certs = rustls_pemfile::certs(&mut reader).collect::<Result<Vec<_>, _>>()?;
    debug!("Successfully parsed {} certificates", certs.len());
    Ok(certs)
}

fn load_private_key(key_path: &Path) -> Result<PrivateKeyDer<'static>, Error> {
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
        error!("No private keys found in key file");
        Err(Error::msg("No private keys found in key file"))
    }
}

mod danger {
    use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
    use rustls::pki_types::UnixTime;
    use rustls::pki_types::{CertificateDer, ServerName};
    use rustls::DigitallySignedStruct;

    #[derive(Debug)]
    pub struct NoCertificateVerification;

    impl ServerCertVerifier for NoCertificateVerification {
        fn verify_server_cert(
            &self,
            _end_entity: &CertificateDer,
            _intermediates: &[CertificateDer],
            _server_name: &ServerName,
            _scts: &[u8],
            _now: UnixTime,
        ) -> Result<ServerCertVerified, rustls::Error> {
            Ok(ServerCertVerified::assertion())
        }

        fn verify_tls12_signature(
            &self,
            _message: &[u8],
            _cert: &CertificateDer,
            _dss: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, rustls::Error> {
            Ok(HandshakeSignatureValid::assertion())
        }

        fn verify_tls13_signature(
            &self,
            _message: &[u8],
            _cert: &CertificateDer,
            _dss: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, rustls::Error> {
            Ok(HandshakeSignatureValid::assertion())
        }


        fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
            vec![
                rustls::SignatureScheme::RSA_PSS_SHA256,
                rustls::SignatureScheme::RSA_PSS_SHA384,
                rustls::SignatureScheme::RSA_PSS_SHA512,
                rustls::SignatureScheme::RSA_PKCS1_SHA256,
                rustls::SignatureScheme::RSA_PKCS1_SHA384,
                rustls::SignatureScheme::RSA_PKCS1_SHA512,
                rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
                rustls::SignatureScheme::ECDSA_NISTP384_SHA384,
                rustls::SignatureScheme::ECDSA_NISTP521_SHA512,
                rustls::SignatureScheme::ED25519,
            ]
        }
    }
}
