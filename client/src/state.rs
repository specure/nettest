use anyhow::Result;
use bytes::{BytesMut};
use log::{debug, info, trace, error};
use mio::{net::TcpStream, Events, Interest, Poll, Token};
use std::{
    net::SocketAddr,
    time::{Duration, Instant},
    io,
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
    PutNoResultReceiveAccept,
    PutNoResultSendCommand,
    PutNoResultReceiveOk,
    PutNoResultSendChunks,
    PutNoResultReceiveTime,
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
        let events = Events::with_capacity(2048);
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

            self.poll.poll(&mut self.events, Some(Duration::from_secs(60)))?;
            // debug!("[run_measurement] event count: {}", self.events.iter().count());

            // Process one event at a time
            if let Some(event) = self.events.iter().next() {
                let needs_write = event.is_writable();
                let needs_read = event.is_readable();

                if needs_write {
                    // debug!("Processing writable event");
                    if let Some(handler) = handler_factory.get_handler(&self.phase) {
                        if let Err(e) = handler.on_write(&mut self.stream, &self.poll) {
                            error!("Error in on_write for phase {:?}: {}", self.phase, e);
                            return Err(e);
                        }
                    }
                }

                if needs_read {
                    // debug!("Processing readable event");
                    if let Some(handler) = handler_factory.get_handler(&self.phase) {
                        if let Err(e) = handler.on_read(&mut self.stream, &self.poll) {
                            error!("Error in on_read for phase {:?}: {}", self.phase, e);
                            return Err(e);
                        }
                        self.phase = handler.get_phase();
                    }
                }

            }
        }

        debug!("[run_measurement] Finished. Final phase: {:?}", self.phase);
        Ok(())
    }
}
