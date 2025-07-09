use crate::client::{
    handlers::BasicHandler, read_until, state::TestPhase, utils::ACCEPT_GETCHUNKS_STRING,
    write_all_nb, MeasurementState, Stream, DEFAULT_READ_BUFFER_SIZE, DEFAULT_WRITE_BUFFER_SIZE,
};
use anyhow::Result;
use bytes::BytesMut;
use log::{debug, trace};
use mio::{Interest, Poll, Token};
use std::io;

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
                trace!(
                    "[on_read] Receiving greeting {}",
                    String::from_utf8_lossy(&self.read_buffer)
                );
                let mut a = read_until(stream, &mut self.read_buffer, "ACCEPT TOKEN QUIT\n")?;
                while !a {
                    a = read_until(stream, &mut self.read_buffer, "ACCEPT TOKEN QUIT\n")?;
                    if a {
                        break;
                    }
                }
                measurement_state.phase = TestPhase::GreetingSendToken;
                stream.reregister(&poll, self.token, Interest::WRITABLE)?;
                trace!("Greeting received 1111: {}", String::from_utf8_lossy(&self.read_buffer));
                self.read_buffer.clear();
                return Ok(());
            }
            TestPhase::GreetingReceiveResponse => {
                trace!(
                    "[on_read] Receiving response {}",
                    String::from_utf8_lossy(&self.read_buffer)
                );
                let mut temp = vec![0; 1024 * 1024]; //TODO understand why 1024 is not enough

                loop {
                    match stream.read(&mut temp) {
                        Ok(n) => {
                            if n == 0 {
                                return Err(anyhow::anyhow!("Error reading greeting: EOF"));
                            }
                            self.read_buffer.extend_from_slice(&temp[0..n]);
                            if String::from_utf8_lossy(&self.read_buffer).contains(ACCEPT_GETCHUNKS_STRING) {
                                measurement_state.phase = TestPhase::GreetingCompleted;
                                stream.reregister(&poll, self.token, Interest::READABLE)?;
                                //remove ok\n from buffer
                                self.read_buffer.clear();
                                return Ok(());
                            }

                            trace!(
                                "Greeting received: {}",
                                String::from_utf8_lossy(&self.read_buffer)
                            );
                        }
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                            trace!("Greeting received would block");
                            stream.reregister(&poll, self.token, Interest::READABLE)?;

                            return Ok(());
                        }

                        Err(e) => {
                            return Err(anyhow::anyhow!("Error reading greeting: {}", e));
                        }
                    }
                }
            }

            _ => {
                return Ok(());
            }
        }
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
                    self.write_buffer.extend_from_slice(&stream.get_greeting());
                }

                match stream {
                    Stream::WebSocketTls(stream) => {
                        debug!("[on_write] WebSocketTls greeting sent",);

                        measurement_state.phase = TestPhase::GreetingSendToken;
                        stream.reregister(&poll, self.token, Interest::WRITABLE)?;
                        self.read_buffer.clear();
                    }
                    Stream::WebSocket(stream) => {
                        debug!("[on_write] WebSocket greeting sent",);

                        measurement_state.phase = TestPhase::GreetingSendToken;
                        stream.reregister(&poll, self.token, Interest::WRITABLE)?;
                        self.read_buffer.clear();
                    }
                    _ => {
                        if write_all_nb(&mut self.write_buffer, stream)? {
                            debug!("[on_write] Writing connection 2 type command");
                            stream.reregister(&poll, self.token, Interest::READABLE)?;
                            measurement_state.phase = TestPhase::GreetingReceiveGreeting;
                        }
                    }
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
                    measurement_state.phase = TestPhase::GreetingReceiveResponse;
                    return Ok(());
                }
                stream.reregister(&poll, self.token, Interest::WRITABLE)?;
            }
            _ => {}
        }
        Ok(())
    }
}
