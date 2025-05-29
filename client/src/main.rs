use log::{info};
use std::{net::SocketAddr};

mod state;
mod handlers;
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
    info!("Connecting to server...");

    // let addr = "10.35.2.151:8081".parse::<SocketAddr>()?;
    let addr = "127.0.0.1:8080".parse::<SocketAddr>()?;

    info!("Connected to server at {}", addr);

    let mut state = TestState::new(addr)?;
    state.run_measurement()?;

    
    info!("Test completed:");

    Ok(())
}
