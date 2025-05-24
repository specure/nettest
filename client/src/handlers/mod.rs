use crate::stream::Stream;
use log::{info};
use anyhow::Result;

pub mod download;

pub async fn handle_download(
    stream: &mut Stream,
) -> Result<()> {
    download::handle_get_chunks(stream).await
}
