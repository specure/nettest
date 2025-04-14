use std::error::Error;
use regex::Regex;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use crate::config::constants::*;
use std::str::FromStr;

#[derive(Debug, Clone)]
pub struct Token {
    pub time: String,
    pub hmac: String,
}

impl FromStr for Token {
    type Err = Box<dyn Error + Send + Sync>;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = s.split(':').collect();
        if parts.len() != 2 {
            return Err("Invalid token format".into());
        }

        Ok(Token {
            time: parts[0].to_string(),
            hmac: parts[1].to_string(),
        })
    }
}

impl ToString for Token {
    fn to_string(&self) -> String {
        format!("{}:{}", self.time, self.hmac)
    }
}

pub fn parse_token(input: &str) -> Result<Token, Box<dyn Error + Send + Sync>> {
    let re = Regex::new(TOKEN_PATTERN)?;
    let caps = re.captures(input).ok_or("Invalid token format")?;
    
    Ok(Token {
        time: caps[2].to_string(),
        hmac: caps[3].to_string(),
    })
}

pub async fn confirm_token_accepted(socket: &mut TcpStream) -> Result<(), Box<dyn Error + Send + Sync>> {
    socket.write_all(ACCEPT_TOKEN.as_bytes()).await?;
    socket.write_all(ACCEPT_COMMANDS.as_bytes()).await?;
    Ok(())
} 