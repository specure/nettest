use anyhow::{Error, Result};
use log::{debug, error, info, trace};
use mio::{net::TcpStream, Interest, Poll, Token, Events};
use rustls::{ClientConfig, ClientConnection, RootCertStore};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName};
use core::error;
use std::fs;
use std::io::{self, BufReader, Read, Write};
use std::net::SocketAddr;
use std::sync::Arc;
use std::path::Path;

use crate::utils::MAX_CHUNK_SIZE;

#[derive(Debug)]
pub struct RustlsStream {
    pub conn: ClientConnection,
    pub stream: TcpStream,
    handshake_done: bool,
}

impl RustlsStream {
    pub fn new(addr: SocketAddr, cert_path: Option<&Path>, key_path: Option<&Path>) -> Result<Self> {
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

        // let mut poll = Poll::new()?;
        // let mut events = Events::with_capacity(8);
        
        // // Register for both read and write events
        // poll.registry().register(&mut stream, Token(0), Interest::READABLE.add(Interest::WRITABLE))?;

        // let mut handshake_done = false;

        // while !handshake_done {
        //     debug!("Polling for events");
        //     poll.poll(&mut events, None)?;

        //     for event in events.iter() {
        //         if event.is_writable() || event.is_readable() {
        //             // Read any pending TLS data
        //             match conn.read_tls(&mut stream) {
        //                 Ok(0) => {
        //                     trace!("TLS connection closed during handshake");
        //                     return Err(Error::msg("TLS connection closed during handshake"));
        //                 }
        //                 Ok(_) => {
        //                     // Process any new TLS messages
        //                     if let Err(e) = conn.process_new_packets() {
        //                         trace!("Error processing new packets during handshake: {:?}", e);
        //                         return Err(Error::from(e));
        //                     }
        //                 }
        //                 Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
        //                     continue;
        //                 }
        //                 Err(e) => {
        //                     trace!("TLS read error during handshake: {:?}", e);
        //                     return Err(Error::from(e));
        //                 }
        //             }

        //             // Write any pending TLS data
        //             while conn.wants_write() {
        //                 if let Err(e) = conn.write_tls(&mut stream) {
        //                     if e.kind() == io::ErrorKind::WouldBlock {
        //                         break;
        //                     }
        //                     trace!("TLS write error during handshake: {:?}", e);
        //                     return Err(Error::from(e));
        //                 }
        //             }

        //             // Check if handshake is complete
        //             if !conn.is_handshaking() {
        //                 debug!("Handshake complete");
        //                 handshake_done = true;
        //                 break;
        //             }
        //         }
        //     }
        // }
        // conn.set_buffer_limit(Some(1024 * MAX_CHUNK_SIZE as usize));
        conn.set_buffer_limit(Some(1024 * 1024 * 4));


        
        Ok(Self { conn, stream, handshake_done: false })
    }

    pub fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        // trace!("Reading from RustlsStream");
        
        // Read TLS data
        match self.conn.read_tls(&mut self.stream) {
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                trace!("TLS read would block");
                return Err(io::Error::new(io::ErrorKind::WouldBlock, "TLS read would block"));
            }
            Err(e) => {
                trace!("TLS read error: {:?}", e);
                return Err(e);
            }
            Ok(0) => {
                trace!("TLS connection closed");
                return Ok(0);
            }
            Ok(n) => {
                trace!("Read {} bytes from TLS stream", n);
            }
        }

        // Process any new TLS messages
        let io_state = match self.conn.process_new_packets() {
            Ok(state) => {
                trace!("Processed new packets {:?}", state.plaintext_bytes_to_read());
                state
            },
            Err(e) => {
                trace!("Error processing new packets: {:?}", e);
                return Err(io::Error::new(io::ErrorKind::Other, e));
            }
        };

        // If we need to write handshake data, do it
        // if !self.handshake_done {
        while self.conn.wants_write() {
            trace!("Writing handshake data");
            self.conn.write_tls(&mut self.stream)?;
        }
        self.handshake_done = true;
    // }

        // Read any new plaintext
        if io_state.plaintext_bytes_to_read() > 0 {
            let n = self.conn.reader().read(buf)?;
            trace!("Read {} bytes of new data", n);
            return Ok(n);
        }

        // If we need to read more data, try again
        if self.conn.wants_read() {
            trace!("Need to read more data");
            return self.read(buf);
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
        trace!("Writing {} bytes to RustlsStream", buf.len());
        
        while total_written < buf.len() {
            match self.conn.writer().write(&buf[total_written..]) {
                Ok(n) => {
                    total_written += n;
                    // Try to send data to network
                    while self.conn.wants_write() {
                        
                        match self.conn.write_tls(&mut self.stream) {
                            Ok(_) => continue,
                            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                                trace!("Write would block {}", total_written);
                                return Err(io::Error::new(io::ErrorKind::WouldBlock, "TLS write would block"));
                            }
                            Err(e) => {
                                trace!("Write error: {:?}", e);
                                return Err(e);
                            }
                        }
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    trace!("Writer would block");
                    return Err(io::Error::new(io::ErrorKind::WouldBlock, "TLS write would block"));
                }
                Err(e) => {
                    trace!("Writer error: {:?}", e);
                    return Err(e);
                }
            }
        }

        trace!("Wrote {} bytes total", total_written);
        Ok(total_written)
    }

    pub fn register(&mut self, poll: &Poll, token: Token, interests: Interest) -> io::Result<()> {
        poll.registry().register(&mut self.stream, token, interests)
    }

    pub fn reregister(&mut self, poll: &Poll, token: Token, interests: Interest) -> io::Result<()> {
        poll.registry().reregister(&mut self.stream, token, interests)
    }

    pub fn perform_handshake(&mut self) -> Result<()> {
       
        info!("TLS handshake completed successfully");
        Ok(())
    }
}

fn load_certs(cert_path: &Path) -> Result<Vec<CertificateDer<'static>>, Error> {
    let cert_path = cert_path.to_str().unwrap();
    info!("Loading certificates from {}", cert_path);
    let certfile = fs::read(cert_path)?;
    info!("Read {} bytes from certificate file", certfile.len());
    let mut reader = BufReader::new(certfile.as_slice());
    let certs = rustls_pemfile::certs(&mut reader).collect::<Result<Vec<_>, _>>()?;
    info!("Successfully parsed {} certificates", certs.len());
    for (i, cert) in certs.iter().enumerate() {
        info!("Certificate {}: {:?}", i, cert);
    }
    Ok(certs)
}

fn load_private_key(key_path: &Path) -> Result<PrivateKeyDer<'static>, Error> {
    let key_path = key_path.to_str().unwrap();
    info!("Loading private key from {}", key_path);
    let keyfile = fs::read(key_path)?;
    info!("Read {} bytes from key file", keyfile.len());
    let mut reader = BufReader::new(keyfile.as_slice());

    // Try to read any private key format
    if let Some(key) = rustls_pemfile::private_key(&mut reader)? {
        info!("Successfully loaded private key: {:?}", key);
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



