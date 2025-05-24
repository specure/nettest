use anyhow::Result;
use handlers::handle_download;
use log::{debug, info};
use mio::{net::TcpStream, Events, Interest, Poll, Token};
use std::io::{Read, Write};
use std::{io, net::SocketAddr, time::Duration};

use bytes::{Buf, BytesMut};
use tokio::time::timeout;

mod connector;
mod handlers;
mod stream;
use handlers::download::GetChunksHandler;

#[tokio::main]
async fn main() {
    env_logger::init();
    if let Err(e) = async_main().await {
        eprintln!("Error: {e:?}");
    }
}

async fn async_main() -> anyhow::Result<()> {
    info!("Starting measurement client...");
    let addr = "127.0.0.1:8080".parse::<SocketAddr>()?;
    let mut stream = TcpStream::connect(addr)?;
    stream.set_nodelay(true)?;

    let mut write_buffer = BytesMut::with_capacity(8192);
    let mut read_buffer = BytesMut::with_capacity(8192);
    const MAX_BUFFER_SIZE: usize = 1024 * 1024; // 1MB максимум
    let mut poll = Poll::new()?;
    let mut events = Events::with_capacity(1024);
    let mut token = Token(0);

    poll.registry()
        .register(&mut stream, token, Interest::READABLE | Interest::WRITABLE)?;

    stream.set_nodelay(true)?;

    // Подготавливаем реквест
    let upgrade_request = "GET /rmbt HTTP/1.1 \r\n\
    Connection: Upgrade \r\n\
    Upgrade: RMBT\r\n\
    RMBT-Version: 1.2.0\r\n\
    \r\n";

    // Добавляем реквест в буфер записи
    write_buffer.extend_from_slice(upgrade_request.as_bytes());
    info!("Prepared upgrade request: {}", upgrade_request);

    loop {
        poll.poll(&mut events, None)?;

        // info!("Events: {:?}", events);
        for event in events.iter() {
            if event.is_readable() {
                let mut buf = [0u8; 8192];
                match stream.read(&mut buf) {
                    Ok(0) => {
                        // Проверяем, действительно ли соединение закрыто
                        if stream.peer_addr().is_err() {
                            info!("Connection closed by peer");
                            break;
                        }
                        // Если соединение живо, продолжаем
                        continue;
                    }
                    Ok(n) => {
                        info!("Received {} bytes", n);
                        read_buffer.extend_from_slice(&buf[..n]);

                        // Обрабатываем строки
                        while let Some(pos) = read_buffer[..].iter().position(|&b| b == b'\n') {
                            let line = read_buffer.split_to(pos + 1);
                            let line_str = String::from_utf8_lossy(&line[..line.len() - 1]);
                            info!("Received line: {}", line_str);

                            // Здесь можно добавить обработку конкретных команд
                            if line_str.contains("RMBTv") {
                                info!("Got greeting: {}", line_str);
                            } else if line_str.contains("ACCEPT TOKEN") {
                                info!("Got token request: {}", line_str);
                                poll.registry().reregister(
                                    &mut stream,
                                    token,
                                    Interest::WRITABLE, 
                                )?;
                                let token = "TEST_TOKEN\n"; // Добавляем \n для протокола
                                info!("Sending token: {}", token);
                                
                                write_buffer.extend_from_slice(token.as_bytes());
                            } else if line_str.contains("CHUNKSIZE") {
                                info!("Got chunk size: {}", line_str);
                            } 

                        }
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                        info!("WouldBlock on read");
                        continue;
                    }
                    Err(e) => {
                        info!("Read error: {}", e);
                        return Err(e.into());
                    }
                }
            }

            if event.is_writable() && !write_buffer.is_empty() {
                match stream.write(&write_buffer) {
                    Ok(0) => {
                        // Проверяем, действительно ли соединение закрыто
                        if stream.peer_addr().is_err() {
                            info!("Connection closed by peer");
                            break;
                        }
                        // Если соединение живо, продолжаем
                        continue;
                    }
                    Ok(n) => {
                        info!("Sent {} bytes", n);
                        write_buffer.advance(n);
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                        info!("WouldBlock on write");
                        continue;
                    }
                    Err(e) => {
                        info!("Write error: {}", e);
                        return Err(e.into());
                    }
                }
            }
            if write_buffer.is_empty() {
                poll.registry().reregister(
                    &mut stream,
                    token,
                    Interest::READABLE, // ⛔️ WRITABLE больше не нужен
                )?;
            }
        }
    }

    let upgrade_request = "GET /rmbt HTTP/1.1 \r\n\
    Connection: Upgrade \r\n\
    Upgrade: RMBT\r\n\
    RMBT-Version: 1.2.0\r\n\
    \r\n";

    // info!("Sending RMBT upgrade request: {}", upgrade_request);
    // stream.write_all(upgrade_request.as_bytes()).await?;

    // // Read and verify upgrade response with timeout
    // info!("Waiting for upgrade response...");
    // let response_str = String::from_utf8_lossy(&self.buffer[..n]);
    // info!("Received RMBT upgrade response: {}", response_str);

    // if !response_str.contains("Upgrade: RMBT") {
    //     return Err(io::Error::new(
    //         io::ErrorKind::InvalidData,
    //         "Invalid upgrade response",
    //     ));
    // }

    // // Read greeting message with timeout
    // info!("Waiting for greeting message...");
    // let n = timeout(Duration::from_secs(5), self.stream.read(&mut self.buffer)).await??;
    // let greeting_str = String::from_utf8_lossy(&self.buffer[..n]);
    // info!("Received greeting: {}", greeting_str);
    // if !greeting_str.contains("RMBTv") {
    //     return Err(io::Error::new(
    //         io::ErrorKind::InvalidData,
    //         "Invalid greeting",
    //     ));
    // }

    // // Read ACCEPT TOKEN message with timeout
    // info!("Waiting for ACCEPT TOKEN message...");
    // let n = timeout(Duration::from_secs(5), self.stream.read(&mut self.buffer)).await??;
    // let accept_token_str = String::from_utf8_lossy(&self.buffer[..n]);
    // info!("Received ACCEPT TOKEN message: {}", accept_token_str);
    // if !accept_token_str.contains("ACCEPT TOKEN") {
    //     return Err(io::Error::new(
    //         io::ErrorKind::InvalidData,
    //         "Invalid ACCEPT TOKEN message",
    //     ));
    // }

    // // Send token
    // let token = "TEST_TOKEN";
    // info!("Sending token: {}", token);
    // self.stream.write_all(token.as_bytes()).await?;

    // // Read token response with timeout
    // info!("Waiting for token response...");
    // let n = timeout(Duration::from_secs(5), self.stream.read(&mut self.buffer)).await??;
    // let response_str = String::from_utf8_lossy(&self.buffer[..n]);
    // info!("Received token response: {}", response_str);
    // if !response_str.contains("OK") {
    //     return Err(io::Error::new(
    //         io::ErrorKind::InvalidData,
    //         "Token not accepted",
    //     ));
    // }

    // // Read CHUNKSIZE message with timeout
    // info!("Waiting for CHUNKSIZE message...");
    // let n = timeout(Duration::from_secs(5), self.stream.read(&mut self.buffer)).await??;
    // let chunksize_str = String::from_utf8_lossy(&self.buffer[..n]);
    // info!("Received CHUNKSIZE message: {}", chunksize_str);
    // if !chunksize_str.contains("CHUNKSIZE") {
    //     return Err(io::Error::new(
    //         io::ErrorKind::InvalidData,
    //         "Invalid CHUNKSIZE message",
    //     ));
    // }

    // // Parse chunk size
    // let chunk_size = chunksize_str
    //     .split_whitespace()
    //     .nth(1)
    //     .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Failed to parse chunk size"))?
    //     .parse::<u32>()
    //     .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    // connector.connect_rmbt().await?;
    // handle_download(&mut connector.stream).await?;
    Ok(())
}
