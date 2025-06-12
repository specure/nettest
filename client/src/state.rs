use anyhow::Result;
use bytes::BytesMut;
use log::{error, info};
use mio::{Events, Interest, Poll, Token};
use std::{
    net::SocketAddr, path::Path, time::Duration
};

use crate::DEFAULT_READ_BUFFER_SIZE;
use crate::{handlers::handler_factory::HandlerFactory};
use crate::stream::Stream;


#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TestPhase {
    GreetingSendConnectionType,
    GreetingSendToken,
    GreetingReceiveGreeting,
    GreetingReceiveResponse,
    GreetingCompleted,

    GetChunksSendChunksCommand,
    GetChunksReceiveChunk,
    GetChunksSendOk,
    GetChunksReceiveTime,
    GetChunksCompleted,


    PingSendPing,
    PingReceivePong,
    PingSendOk,
    PingReceiveTime,
    PingCompleted,


    GetTimeSendCommand,
    GetTimeReceiveChunk,
    GetTimeSendOk,
    GetTimeReceiveTime,
    GetTimeCompleted,


    PutNoResultSendCommand,
    PutNoResultReceiveOk,
    PutNoResultSendChunks,
    PutNoResultReceiveTime,
    PutNoResultCompleted,

    PutSendCommand,
    PutReceiveOk,
    PutSendChunks,
    PutReceiveBytesTime,
    PutReceiveTime,
    PutCompleted
}

pub struct TestState {
    stream: Stream,
    poll: Poll,
    events: Events,
    token: Token,
}

pub struct MeasurementState {
   pub phase: TestPhase,
   pub buffer: BytesMut,
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
    pub fn new(addr: SocketAddr, use_tls: bool, cert_path: Option<&Path>, key_path: Option<&Path>) -> Result<Self> {
        let mut poll = Poll::new()?;
        let events = Events::with_capacity(2048);
        let token = Token(0);

       

        let mut stream = if use_tls {
                Stream::new_openssl(addr)?
                // Stream::new_rustls(addr, cert_path, key_path)?
        } else {
            Stream::new_tcp(addr)?
        };



        stream.register(&mut poll, token, Interest::READABLE | Interest::WRITABLE)?;


        Ok(Self {
            stream,
            poll,
            events,
            token,
        })
    }

    pub fn run_measurement(&mut self) -> Result<MeasurementState> {
        // self.poll
        //     .registry()
        //     .reregister(&mut self.stream, self.token, Interest::WRITABLE)?;
        self.stream.reregister(&mut self.poll, self.token, Interest::WRITABLE | Interest::READABLE)?;

        let mut handler_factory = HandlerFactory::new(self.token)?;

        let mut measurement_state = MeasurementState {
            buffer: BytesMut::with_capacity(DEFAULT_READ_BUFFER_SIZE),
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

        measurement_state = self.process_phase(measurement_state, &mut handler_factory, TestPhase::GreetingCompleted)?;

        info!("Greeting completed");

        measurement_state.phase = TestPhase::GetChunksSendChunksCommand;

        self.stream.reregister(&mut self.poll, self.token, Interest::WRITABLE)?;


        measurement_state = self.process_phase(measurement_state, &mut handler_factory, TestPhase::GetChunksCompleted)?;

        info!("Get chunks completed");

        measurement_state.phase = TestPhase::PingSendPing;

        self.stream.reregister(&mut self.poll, self.token, Interest::WRITABLE)?;


        measurement_state = self.process_phase(measurement_state, &mut handler_factory, TestPhase::PingCompleted)?;


        info!("Ping completed");

        measurement_state.phase = TestPhase::GetTimeSendCommand;

        measurement_state = self.process_phase(measurement_state, &mut handler_factory, TestPhase::GetTimeCompleted)?;


        info!("Get time completed");

        measurement_state.phase = TestPhase::PutNoResultSendCommand;

        self.stream.reregister(&mut self.poll, self.token, Interest::WRITABLE)?;

        measurement_state = self.process_phase(measurement_state, &mut handler_factory, TestPhase::PutNoResultCompleted)?;

        info!("Put no result completed");

        measurement_state.phase = TestPhase::PutSendCommand;

        self.stream.reregister(&mut self.poll, self.token, Interest::WRITABLE)?;

        measurement_state = self.process_phase(measurement_state, &mut handler_factory, TestPhase::PutCompleted)?;
        

        Ok(measurement_state)



    }

    fn process_phase(&mut self, mut measurement_state: MeasurementState, handler_factory: &mut HandlerFactory, phase: TestPhase) -> Result<MeasurementState> {
        while measurement_state.phase != phase {
            self.poll
                .poll(&mut self.events, Some(Duration::from_secs(20)))?;

            // Process events in the current poll iteration
            for event in &self.events {
                // Handle write events first to ensure data is sent
                if event.is_writable() {
                    if let Some(handler) = handler_factory.get_handler(&measurement_state.phase) {
                        if let Err(e) = handler.on_write(&mut self.stream, &self.poll, &mut measurement_state) {
                            error!("Error in on_write for phase {:?}: {}", measurement_state.phase, e);
                            return Err(e);
                        }
                    }
                }

                // Handle read events after write to process any responses
                if event.is_readable() {
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

