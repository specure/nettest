use std::{error::Error, sync::Arc};

use log::debug;
use rustls::{
    client::danger::{ServerCertVerifier, ServerCertVerified, HandshakeSignatureValid},
    ClientConfig, RootCertStore,
    pki_types::{CertificateDer, ServerName, UnixTime},
    Error as RustlsError,
};
use tokio_rustls::TlsConnector;

pub fn load_identity() -> Result<TlsConnector, Box<dyn Error + Send + Sync>> {
    debug!("Creating TLS client configuration without certificate verification");
    
    // Создаем клиентскую конфигурацию с кастомным верификатором
    let config = ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(NoVerifier))
        .with_no_client_auth();

    debug!("TLS client configuration built successfully");
    Ok(TlsConnector::from(Arc::new(config)))
}

#[derive(Debug)]
struct NoVerifier;

impl ServerCertVerifier for NoVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, RustlsError> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        vec![
            // Современные и рекомендуемые схемы
            rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            rustls::SignatureScheme::ECDSA_NISTP384_SHA384,
            rustls::SignatureScheme::ECDSA_NISTP521_SHA512,
            rustls::SignatureScheme::ED25519,
            rustls::SignatureScheme::RSA_PSS_SHA256,
            rustls::SignatureScheme::RSA_PSS_SHA384,
            rustls::SignatureScheme::RSA_PSS_SHA512,
            rustls::SignatureScheme::RSA_PKCS1_SHA256,
            rustls::SignatureScheme::RSA_PKCS1_SHA384,
            rustls::SignatureScheme::RSA_PKCS1_SHA512,
        ]
    }
}

// fn load_certs() -> Result<Vec<CertificateDer<'static>>, Box<dyn Error + Send + Sync>> {
//     let cert_path = self.cert_path.as_ref().unwrap();
//     debug!("Loading certificates from {}", cert_path);
//     let certfile = fs::read(cert_path)?;
//     debug!("Read {} bytes from certificate file", certfile.len());
//     let mut reader = BufReader::new(certfile.as_slice());
//     let certs = rustls_pemfile::certs(&mut reader).collect::<Result<Vec<_>, _>>()?;
//     debug!("Successfully parsed {} certificates", certs.len());
//     Ok(certs)
// }

// fn load_private_key() -> Result<PrivateKeyDer<'static>, Box<dyn Error + Send + Sync>> {
//     let key_path = self.key_path.as_ref().unwrap();
//     debug!("Loading private key from {}", key_path);
//     let keyfile = fs::read(key_path)?;
//     debug!("Read {} bytes from key file", keyfile.len());
//     let mut reader = BufReader::new(keyfile.as_slice());

//     // Try to read any private key format
//     if let Some(key) = rustls_pemfile::private_key(&mut reader)? {
//         debug!("Successfully loaded private key: {:?}", key);
//         Ok(key)
//     } else {
//         debug!("No private keys found in key file");
//         Err("No private keys found in key file".into())
//     }
// }