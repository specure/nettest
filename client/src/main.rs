use log::{info};
use std::{net::SocketAddr, path::Path};

mod state;
mod handlers;
use state::TestState;
pub mod utils;
pub mod stream;
pub mod rustls;

pub use handlers::{
    greeting::GreetingHandler,
    get_chunks::GetChunksHandler,
    put::PutHandler,
    put_no_result::PutNoResultHandler,
};
pub use utils::{read_until, write_all, write_all_nb, DEFAULT_READ_BUFFER_SIZE, DEFAULT_WRITE_BUFFER_SIZE, RMBT_UPGRADE_REQUEST};

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

    // let addr = "10.35.2.151:8082".parse::<SocketAddr>()?;
    let addr = "127.0.0.1:8080".parse::<SocketAddr>()?;

    info!("Connected to server at {}", addr);

    // Используем небезопасную конфигурацию дл я тестирования
    let mut state = TestState::new(addr, true, None, None)?;
    let measurement_state = state.run_measurement()?;

    info!("Measurement completed");
    // info!("Upload results for graph: {:?}", measurement_state.upload_results_for_graph);
    info!("Chunk size: {}", measurement_state.chunk_size);
    info!("Ping median mS: {:?}", measurement_state.ping_median.unwrap() / 1_000_000);
    info!("Upload bytes: {:?}", measurement_state.upload_bytes);
    info!("Upload time: {:?}", measurement_state.upload_time);
    info!("Upload speed GBit/s: {}", (measurement_state.upload_speed.unwrap() * 8.0 / (1024.0 * 1024.0 * 1024.0)).round());
    info!("Download time: {:?}", measurement_state.download_time);
    info!("Download bytes: {:?}", measurement_state.download_bytes);
    info!("Download speed GBit/s: {}", (measurement_state.download_speed.unwrap() * 8.0 / (1024.0 * 1024.0 * 1024.0)).round());

    
    info!("Test completed");

    Ok(())
}
