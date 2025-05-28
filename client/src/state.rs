use anyhow::Result;
use bytes::{BytesMut};
use log::{debug, info, trace};
use mio::{net::TcpStream, Events, Interest, Poll, Token};
use std::{
    net::SocketAddr,
    time::{Duration, Instant},
};

use crate::handlers::handler_factory::HandlerFactory;
use crate::handlers::{GetChunksHandler, GreetingHandler};

const MIN_CHUNK_SIZE: u32 = 4096; // 4KB

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TestPhase {
    GreetingSendConnectionType,
    GreetingSendToken,
    GreetingReceiveGreeting,
    GreetingReceiveVersion,
    GreetingReceiveAcceptToken,
    GreetingReceiveOK,
    GetChunksReceiveAccept,
    GetChunksSendChunksCommand,
    GetChunksReceiveChunk,
    GetChunksSendOk,
    GetChunksReceiveTime,
    PingSendPing,
    PingReceivePong,
    PingSendOk,
    PingReceiveTime,
    GetTimeCommand,
    End,
}

pub struct TestState {
    stream: TcpStream,
    poll: Poll,
    events: Events,
    token: Token,
    phase: TestPhase,
}

impl TestState {
    pub fn new(addr: SocketAddr) -> Result<Self> {
        let mut stream = TcpStream::connect(addr)?;
        stream.set_nodelay(true)?;

        let mut poll = Poll::new()?;
        let events = Events::with_capacity(1024);
        let token = Token(0);

        // Register socket for both reading and writing
        poll.registry()
            .register(&mut stream, token, Interest::READABLE | Interest::WRITABLE)?;

        Ok(Self {
            stream,
            poll,
            events,
            token,
            phase: TestPhase::GreetingSendConnectionType,
        })
    }

    pub fn run_measurement(&mut self) -> Result<()> {
        self.poll
            .registry()
            .reregister(&mut self.stream, self.token, Interest::WRITABLE)?;

        let mut handler_factory = HandlerFactory::new(self.token)?;

        while self.phase != TestPhase::End {
            self.poll.poll(&mut self.events, None)?;
            let mut needs_write = true;
            let mut needs_read = true;

            trace!("[run_measurement] Events: {:?}", self.events);

            for event in self.events.iter() {
                if event.is_readable() {
                    needs_read = true;
                }
                if event.is_writable() {
                    needs_write = true;
                }
            }

            if needs_write {
                if let Some(handler) = handler_factory.get_handler(&self.phase) {
                    handler.on_write(&mut self.stream, &self.poll)?;
                }
            }

            if needs_read {
                if let Some(handler) = handler_factory.get_handler(&self.phase) {
                    handler.on_read(&mut self.stream, &self.poll)?;
                    self.phase = handler.get_phase();
                }
            }
        }

        debug!("[run_measurement] Finished. Final phase: {:?}", self.phase);
        Ok(())
    }
}
