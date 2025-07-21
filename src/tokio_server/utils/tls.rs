use std::error::Error;
use tokio_native_tls::native_tls::{Identity, TlsAcceptor as NativeTlsAcceptor};
use tokio_native_tls::{TlsAcceptor, TlsStream};
use tokio::net::TcpStream;

pub struct TlsConfig {
    acceptor: TlsAcceptor,
}

impl TlsConfig {
    pub fn new(cert_path: &str, key_path: &str) -> Result<Self, Box<dyn Error + Send + Sync>> {
        let cert = std::fs::read(cert_path)?;
        let key = std::fs::read(key_path)?;
        let identity = Identity::from_pkcs8(&cert, &key)?;
        let acceptor = NativeTlsAcceptor::new(identity)?;
        let acceptor = TlsAcceptor::from(acceptor);
        Ok(Self { acceptor })
    }

    pub async fn accept(&self, stream: TcpStream) -> Result<TlsStream<TcpStream>, Box<dyn Error + Send + Sync>> {
        Ok(self.acceptor.accept(stream).await?)
    }
} 