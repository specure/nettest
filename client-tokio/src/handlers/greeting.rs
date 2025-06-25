use std::error::Error;

use log::{debug, info};

use crate::stream::Stream;

pub const RMBT_UPGRADE_REQUEST: &str = "GET /rmbt HTTP/1.1 \r\n\
    Connection: Upgrade \r\n\
    Upgrade: RMBT\r\n\
    RMBT-Version: 1.2.0\r\n\
    \r\n";

pub const WEBSOCKET_UPGRADE_REQUEST: &str = "GET /websocket HTTP/1.1 \r\n\
    Connection: Upgrade \r\n\
    Upgrade: websocket\r\n\
    \r\n";

pub async fn handle_greeting(
    mut stream: &mut Stream,
    is_websocket: bool,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut read_buffer: Vec<u8> = Vec::with_capacity(1024);
    let mut buffer: Vec<u8> = vec![0; 1024];

    if is_websocket {
        // Generate WebSocket key
    } else {
        let commandd = RMBT_UPGRADE_REQUEST.as_bytes().to_vec();
        stream.write(&commandd).await?;

        loop {
            let n = stream.read(&mut buffer).await?;

            if n == 0 {
                debug!("Connection closed");
                break;
            }
            info!("Received: {:?}", String::from_utf8_lossy(&buffer[..n]));

            read_buffer.extend_from_slice(&buffer[..n]);
            let line = String::from_utf8_lossy(&read_buffer);
            debug!("Line: {:?}", line);
            if line.contains(&"ACCEPT TOKEN QUIT\n") {
                break;
            }
        }
    }

    debug!("Sending TOKEN 1");
    let command = "TOKEN 1\n".as_bytes().to_vec();
    stream.write(&command).await?;

    loop {
        let n = stream.read(&mut buffer).await?;
        debug!("Received: {:?}", String::from_utf8_lossy(&buffer[..n]));
        if n == 0 {
            debug!("Connection closed");
            break;
        }
        read_buffer.extend_from_slice(&buffer[..n]);
        let line = String::from_utf8_lossy(&read_buffer);
        debug!("Line: {:?}", line);
        if line.contains(&"ACCEPT GETCHUNKS GETTIME PUT PUTNORESULT PING QUIT\n") {
            break;
        }
    }
    Ok(())
}
