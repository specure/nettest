use anyhow::Result;
use bytes::BytesMut;
use log::{debug, error, info, trace};
use mio::{net::TcpStream, Events, Interest, Poll, Token};
use std::{
    io,
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
    GetChunksReceiveAccept,
    GetChunksSendChunksCommand,
    GetChunksReceiveChunk,
    GetChunksSendOk,
    GetChunksReceiveTime,
    PingSendPing,
    PingReceivePong,
    PingSendOk,
    PingReceiveTime,
    PutNoResultSendCommand,
    PutNoResultReceiveOk,
    PutNoResultSendChunks,
    PutNoResultReceiveTime,
    PutSendCommand,
    PutReceiveOk,
    PutSendChunks,
    PutReceiveBytesTime,
    PutReceiveTime,
    GetTimeSendCommand,
    GetTimeReceiveChunk,
    GetTimeSendOk,
    GetTimeReceiveTime,
    End,
}

pub struct TestState {
    stream: TcpStream,
    poll: Poll,
    events: Events,
    token: Token,
}

pub struct MeasurementState {
   pub phase: TestPhase,
    pub upload_results_for_graph: Vec<(u64, u64)>,
    pub upload_bytes: Option<u64>,
    pub upload_time: Option<u64>,
    pub upload_speed: Option<f64>,
    pub download_time: Option<u64>,
    pub download_bytes: Option<u64>,
    pub download_speed: Option<f64>,
    pub chunk_size: u32,
    pub ping_median: Option<u64>,
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
        })
    }

    pub fn run_measurement(&mut self) -> Result<MeasurementState> {
        self.poll
            .registry()
            .reregister(&mut self.stream, self.token, Interest::WRITABLE)?;

        let mut handler_factory = HandlerFactory::new(self.token)?;

        let mut measurement_state = MeasurementState {
            phase: TestPhase::GreetingSendConnectionType,
            upload_results_for_graph: Vec::new(),
            upload_bytes: None,
            upload_time: None,
            upload_speed: None,
            download_time: None,
            download_bytes: None,
            download_speed: None,
            chunk_size: 0,
            ping_median: None,
        };

        while measurement_state.phase != TestPhase::End {
            self.poll
                .poll(&mut self.events, Some(Duration::from_secs(120)))?;

            // Process one event at a time
            if let Some(event) = self.events.iter().next() {
                let needs_write = event.is_writable();
                let needs_read = event.is_readable();

                if needs_write {
                    if let Some(handler) = handler_factory.get_handler(&measurement_state.phase) {
                        if let Err(e) = handler.on_write(&mut self.stream, &self.poll, &mut measurement_state) {
                            error!("Error in on_write for phase {:?}: {}", measurement_state.phase, e);
                            return Err(e);
                        }
                    }
                }

                if needs_read {
                    if let Some(handler) = handler_factory.get_handler(&measurement_state.phase) {
                        if let Err(e) = handler.on_read(&mut self.stream, &self.poll, &mut measurement_state) {
                            error!("Error in on_read for phase {:?}: {}", measurement_state.phase, e);
                            return Err(e);
                        }
                    }
                }
            }
        }

        Ok(measurement_state)
    }
}
