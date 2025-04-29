use std::error::Error;
use sha1::{Sha1, Digest};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};

const WS_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

#[derive(Debug)]
pub struct Handshake {
    pub host: Option<String>,
    pub origin: Option<String>,
    pub key: Option<String>,
    pub resource: Option<String>,
}

impl Handshake {
    pub fn new() -> Self {
        Handshake {
            host: None,
            origin: None,
            key: None,
            resource: None,
        }
    }

    pub fn parse(request: &str) -> Result<Self, Box<dyn Error + Send + Sync>> {
        let mut handshake = Handshake::new();
        
        // Parse resource path
        if let Some(resource) = request.lines().next().and_then(|line| {
            line.split_whitespace().nth(1)
        }) {
            handshake.resource = Some(resource.to_string());
        }

        // Parse headers
        for line in request.lines().skip(1) {
            if let Some((name, value)) = line.split_once(':') {
                match name.trim().to_lowercase().as_str() {
                    "host" => handshake.host = Some(value.trim().to_string()),
                    "origin" => handshake.origin = Some(value.trim().to_string()),
                    "sec-websocket-key" => handshake.key = Some(value.trim().to_string()),
                    _ => {}
                }
            }
        }

        Ok(handshake)
    }

    pub fn generate_accept_key(&self) -> Result<String, Box<dyn Error + Send + Sync>> {
        let key = self.key.as_ref().ok_or("No WebSocket key found")?;
        let mut hasher = Sha1::new();
        hasher.update(key);
        hasher.update(WS_GUID);
        let result = hasher.finalize();
        Ok(BASE64.encode(result))
    }

    pub fn is_valid(&self) -> bool {
        self.host.is_some() && self.key.is_some()
    }
}

pub fn generate_handshake_response(handshake: &Handshake) -> Result<String, Box<dyn Error + Send + Sync>> {
    let accept_key = handshake.generate_accept_key()?;
    Ok(format!(
        "HTTP/1.1 101 Switching Protocols\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Accept: {}\r\n\
         \r\n",
        accept_key
    ))
} 