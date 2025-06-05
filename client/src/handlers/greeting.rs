use std::io::{self, Write};

use crate::handlers::BasicHandler;
use crate::state::TestPhase;
use crate::utils::{read_until, write_all,write_all_nb, DEFAULT_READ_BUFFER_SIZE, DEFAULT_WRITE_BUFFER_SIZE, RMBT_UPGRADE_REQUEST};
use anyhow::Result;
use bytes::BytesMut;
use log::debug;
use mio::{net::TcpStream, Interest, Poll, Token};

pub struct GreetingHandler {
    token: Token,
    phase: TestPhase,
    read_buffer: BytesMut,  // Buffer for reading responses
    write_buffer: BytesMut, // Buffer for writing requests
}

impl GreetingHandler {
    pub fn new(token: Token) -> Result<Self> {
        Ok(Self {
            token,
            phase: TestPhase::GreetingSendConnectionType,
            read_buffer: BytesMut::with_capacity(DEFAULT_READ_BUFFER_SIZE),
            write_buffer: BytesMut::with_capacity(DEFAULT_WRITE_BUFFER_SIZE),
        })
    }
}

impl BasicHandler for GreetingHandler {
    fn on_read(&mut self, stream: &mut TcpStream, poll: &Poll) -> Result<()> {
        match self.phase {
            TestPhase::GreetingReceiveGreeting => {
                if read_until(stream, &mut self.read_buffer, "ACCEPT TOKEN QUIT\n")? {
                    self.phase = TestPhase::GreetingSendToken;
                    poll.registry()
                        .reregister(stream, self.token, Interest::WRITABLE)?;
                    self.read_buffer.clear();
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn on_write(&mut self, stream: &mut TcpStream, poll: &Poll) -> Result<()> {
        match self.phase {
            TestPhase::GreetingSendConnectionType => {
                if self.write_buffer.is_empty() {
                    self.write_buffer.extend_from_slice(RMBT_UPGRADE_REQUEST.as_bytes());
                }
                if write_all_nb(&mut self.write_buffer, stream)? {
                    poll.registry()
                    .reregister(stream, self.token, Interest::READABLE)?;
                self.phase = TestPhase::GreetingReceiveGreeting;
                }
            }
            TestPhase::GreetingSendToken => {
                if self.write_buffer.is_empty() {
                    debug!("[on_write] Sending token command");
                    self.write_buffer
                        .extend_from_slice(format!("TOKEN {}\n", self.token.0).as_bytes());
                }
                if write_all_nb(&mut self.write_buffer, stream)? {
                    poll.registry()
                    .reregister(stream, self.token, Interest::READABLE)?;
                    self.phase = TestPhase::GetChunksReceiveAccept;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn get_phase(&self) -> TestPhase {
        self.phase.clone()
    }
}
