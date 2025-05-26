use anyhow::Result;
// use handlers::handle_download;
use log::{debug, info};
use mio::{net::TcpStream, Events, Interest, Poll, Token};
use std::io::{Read, Write};
use std::{io, net::SocketAddr, time::Duration};

use bytes::{Buf, BytesMut};
use tokio::time::timeout;

mod state;
mod handlers;
mod stream;
// use handlers::download::GetChunksHandler;
use state::TestState;

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
    
    let mut state = TestState::new(addr)?;
    state.run_measurement()?;

    let bytes_received = state.get_bytes_received();
    let duration = state.get_test_duration();
    let speed_mbps = (bytes_received as f64 * 8.0) / (duration.as_secs_f64() * 1_000_000.0);
    
    info!("Test completed:");
    info!("Bytes received: {}", bytes_received);
    info!("Duration: {:?}", duration);
    info!("Speed: {:.2} Mbps", speed_mbps);

    Ok(())
}
