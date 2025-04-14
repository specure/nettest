pub mod commands;
pub mod token;
pub mod messages;

use std::error::Error;
use std::str::FromStr;
use crate::config::constants::{GREETING, GREETING_V3, ACCEPT_TOKEN, ACCEPT_COMMANDS};
use crate::server::connection_handler::Stream;

pub use token::Token;
pub use messages::read_line;

pub fn parse_token(token_str: &str) -> Result<Token, Box<dyn Error + Send + Sync>> {
    Token::from_str(token_str)
}

pub async fn send_greeting(stream: &mut Stream, version: u8) -> Result<(), Box<dyn Error + Send + Sync>> {
    let greeting = if version == 3 { GREETING_V3 } else { GREETING };
    stream.write_all(greeting.as_bytes()).await?;
    Ok(())
}

pub async fn confirm_token_accepted(stream: &mut Stream) -> Result<(), Box<dyn Error + Send + Sync>> {
    stream.write_all(ACCEPT_TOKEN.as_bytes()).await?;
    stream.write_all(ACCEPT_COMMANDS.as_bytes()).await?;
    Ok(())
}

pub async fn send_response(stream: &mut Stream, response: &str) -> Result<(), Box<dyn Error + Send + Sync>> {
    stream.write_all(response.as_bytes()).await?;
    Ok(())
} 