use crate::config::constants::{RESP_BYE, RESP_PONG};
use crate::server::connection_handler::Stream;
use std::error::Error;

mod get_time;
mod get_chunks;
mod put;
mod put_no_result;

pub async fn handle_get_time(
    stream: &mut Stream,
    command: &str,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    get_time::handle_get_time(stream, command).await
}

pub async fn handle_get_chunks(stream: &mut Stream,  command: &str,) -> Result<(), Box<dyn Error + Send + Sync>> {
   get_chunks::handle_get_chunks(stream, command).await
}

pub async fn handle_put(stream: &mut Stream,  command: &str,) -> Result<(), Box<dyn Error + Send + Sync>> {
   put::handle_put(stream, command).await
}

pub async fn handle_put_no_result(stream: &mut Stream,  command: &str,) -> Result<(), Box<dyn Error + Send + Sync>> {
    put_no_result::handle_put_no_result(stream, command).await
}

pub async fn handle_ping(stream: &mut Stream) -> Result<(), Box<dyn Error + Send + Sync>> {
    stream.write_all(RESP_PONG.as_bytes()).await?;
    Ok(())
}

pub async fn handle_quit(stream: &mut Stream) -> Result<(), Box<dyn Error + Send + Sync>> {
    stream.write_all(RESP_BYE.as_bytes()).await?;
    Ok(())
} 