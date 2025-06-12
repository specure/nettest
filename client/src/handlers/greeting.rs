use std::io::{self, Write};

use crate::handlers::BasicHandler;
use crate::state::{MeasurementState, TestPhase};
use crate::utils::{
    read_until, write_all, write_all_nb, DEFAULT_READ_BUFFER_SIZE, DEFAULT_WRITE_BUFFER_SIZE,
    RMBT_UPGRADE_REQUEST,
};
use crate::stream::Stream;
use anyhow::Result;
use bytes::BytesMut;
use log::debug;
use mio::{net::TcpStream, Interest, Poll, Token};

pub struct GreetingHandler {
    token: Token,
    read_buffer: BytesMut,  // Buffer for reading responses
    write_buffer: BytesMut, // Buffer for writing requests
}

impl GreetingHandler {
    pub fn new(token: Token) -> Result<Self> {
        Ok(Self {
            token,
            read_buffer: BytesMut::with_capacity(DEFAULT_READ_BUFFER_SIZE),
            write_buffer: BytesMut::with_capacity(DEFAULT_WRITE_BUFFER_SIZE),
        })
    }
}

impl BasicHandler for GreetingHandler {
    fn on_read(
        &mut self,
        stream: &mut Stream,
        poll: &Poll,
        measurement_state: &mut MeasurementState,
    ) -> Result<()> {
        match measurement_state.phase {
            TestPhase::GreetingReceiveGreeting => {
                debug!("[on_read] Receiving greeting {}", String::from_utf8_lossy(&self.read_buffer));
                if read_until(stream, &mut self.read_buffer, "ACCEPT TOKEN QUIT\n")? {
                    measurement_state.phase = TestPhase::GreetingSendToken;
                    stream.reregister(&poll, self.token, Interest::WRITABLE)?;
                    self.read_buffer.clear();
                }
                // else {
                //     self.on_read(stream, poll, measurement_state)?;
                // }
            }
            _ => {}
        }
        Ok(())
    }

    fn on_write(
        &mut self,
        stream: &mut Stream,
        poll: &Poll,
        measurement_state: &mut MeasurementState,
    ) -> Result<()> {
        match measurement_state.phase {
            TestPhase::GreetingSendConnectionType => {
                debug!("[on_write] Sending connection type command");
                if self.write_buffer.is_empty() {
                    debug!("[on_write] Writing connection 1 type command");
                    self.write_buffer
                        .extend_from_slice(RMBT_UPGRADE_REQUEST.as_bytes());
                }
                if write_all_nb(&mut self.write_buffer, stream)? {
                    debug!("[on_write] Writing connection 2 type command");
                    stream.reregister(&poll, self.token, Interest::READABLE)?;
                    measurement_state.phase = TestPhase::GreetingReceiveGreeting;
                }
            }
            TestPhase::GreetingSendToken => {
                if self.write_buffer.is_empty() {
                    debug!("[on_write] Sending token command");
                    self.write_buffer
                        .extend_from_slice(format!("TOKEN {}\n", self.token.0).as_bytes());
                }
                if write_all_nb(&mut self.write_buffer, stream)? {
                    debug!("[on_write] Writing token command");
                    stream.reregister(&poll, self.token, Interest::READABLE)?;
                    measurement_state.phase = TestPhase::GetChunksReceiveAccept;
                    return Ok(());
                }
                stream.reregister(&poll, self.token, Interest::WRITABLE)?;
            }
            _ => {}
        }
        Ok(())
    }
}
