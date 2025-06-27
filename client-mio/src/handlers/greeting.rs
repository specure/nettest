use std::{error::Error, net::TcpStream};

use log::{debug, info};
use mio::{Events, Interest, Poll, Token};


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
    mut stream: &mut TcpStream,
    is_websocket: bool,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut read_buffer: Vec<u8> = Vec::with_capacity(1024);
    let mut buffer: Vec<u8> = vec![0; 1024];

        let commandd = RMBT_UPGRADE_REQUEST.as_bytes().to_vec();


        let mut poll = Poll::new()?;
        let mut events = Events::with_capacity(2048);
        let token = Token(1);


        stream.register(&mut poll, token, Interest::WRITABLE)?;


        loop {
            poll.poll(&mut events, None)?;
            for event in events.iter() {
                if event.is_writable() {
                    match stream.write(&commandd) {
                        Ok(n) => {
                            debug!("Sent {} bytes", n);
                            break;
                        }
                        Err(e) => {
                            debug!("Error writing: {}", e);
                    }
                }
            }
        }
    }

    stream.register(&mut poll, token, Interest::READABLE)?;


        loop {
            poll.poll(&mut events, None)?;
            for event in events.iter() {
            match stream.read(&mut buffer) {
                Ok(n) => {
                    debug!("Received {} bytes", n);
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
                    break;
                }
                Err(e) => {
                    debug!("Error reading: {}", e);

    
        }}
    }
}

    stream.register(&mut poll, token, Interest::WRITABLE)?;


  loop {
    poll.poll(&mut events, None)?;
    for event in events.iter() {
      if event.is_writable() {
        may
      }
    }
  }






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
}
