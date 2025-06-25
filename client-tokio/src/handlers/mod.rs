use crate::config::constants::{RESP_BYE};
use crate::stream::Stream;
use crate::MeasurementResult;
use std::error::Error;

mod greeting;
mod get_time;
mod put_no_result;

pub async fn handle_greeting(stream: &mut Stream, is_websocket: bool) -> Result<(), Box<dyn Error + Send + Sync>> {
    greeting::handle_greeting(stream, is_websocket).await
}



pub async fn handle_get_time(
    stream: &mut Stream,
    result: &mut MeasurementResult,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    get_time::handle_get_time(stream, result).await
}

// pub async fn handle_get_chunks(
//     stream: &mut Stream,
//     command: &str,
// ) -> Result<(), Box<dyn Error + Send + Sync>> {
//     get_chunks::handle_get_chunks(stream, command).await
// }

pub async fn handle_put_no_result(
    stream: &mut Stream,
    result: &mut MeasurementResult,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    put_no_result::handle_put_no_result(stream, result).await
}


// pub async fn handle_ping(
//     stream: &mut Stream,
// ) -> Result<(), Box<dyn Error + Send + Sync>> {
//     ping::handle_ping(stream).await
// }

// pub async fn handle_put_no_result(
//     stream: &mut Stream,
//     command: &str,
// ) -> Result<(), Box<dyn Error + Send + Sync>> {
//     put_no_result::handle_put_no_result(stream, command).await
// }

pub async fn handle_quit(stream: &mut Stream) -> Result<(), Box<dyn Error + Send + Sync>> {
    // Send BYE to client
    stream.write_all(RESP_BYE.as_bytes()).await?;

    Ok(())
}

