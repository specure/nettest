use anyhow::Result;
use bytes::BytesMut;
use log::{debug, info, trace};
use mio::{Events, Interest, Poll, Token};
use std::collections::{HashMap, VecDeque};
use std::time::Instant;
use std::{net::SocketAddr, path::Path, time::Duration};

use crate::handlers::handler_factory::HandlerFactory;
use crate::stream::Stream;
use crate::DEFAULT_READ_BUFFER_SIZE;

const ONE_SECOND_NS: u128 = 1_000_000_000;

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

    PerfNoResultSendCommand,
    PerfNoResultReceiveOk,
    PerfNoResultSendChunks,
    PerfNoResultSendLastChunk,

    PerfNoResultReceiveTime,
    PerfNoResultCompleted,

    PutSendCommand,
    PutReceiveOk,
    PutSendChunks,
    PutReceiveBytesTime,
    PutReceiveTime,
    PutQuit,
    PutCompleted,
}

pub struct TestState {
    stream: Stream,
    poll: Poll,
    events: Events,
    token: Token,
    measurement_state: MeasurementState,
    handler_factory: HandlerFactory,
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
    pub chunk_size: usize,
    pub ping_median: Option<u64>,
    pub phase_start_time: Option<Instant>,
    pub read_buffer_temp: Vec<u8>,
    pub measurements: VecDeque<(u64, u64)>, // Хранит (t_k^(j), b_k^(j)) для каждого чанка\
    pub upload_measurements: VecDeque<(u64, u64)>, // Хранит (t_k^(j), b_k^(j)) для каждого чанка\
    pub failed: bool,
}

impl TestState {
    pub fn new(
        addr: SocketAddr,
        use_tls: bool,
        use_websocket: bool,
        tok: usize,
        cert_path: Option<&Path>,
        key_path: Option<&Path>,
    ) -> Result<Self> {
        let mut poll = Poll::new()?;
        let events = Events::with_capacity(2048);
        let token = Token(tok);

        let mut stream = if use_tls && use_websocket {
            Stream::new_websocket_tls(addr)?
            
        } else if use_tls {
            // Stream::new_openssl_sys(addr)?
            // Stream::new_openssl(addr)?
            Stream::new_rustls(addr, cert_path, key_path)?
        } else {
            if use_websocket {
                Stream::new_websocket(addr)?
            } else {
                Stream::new_tcp(addr)?
            }
        };

        stream.register(&mut poll, token, Interest::READABLE | Interest::WRITABLE)?;

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
            read_buffer_temp: vec![0u8; 1024 * 1024 * 10],
            measurements: VecDeque::new(),
            upload_measurements: VecDeque::new(),
            phase_start_time: None,
            failed: false,
        };

        let mut handler_factory: HandlerFactory = HandlerFactory::new(token)?;

        Ok(Self {
            stream,
            poll,
            events,
            token,
            measurement_state,
            handler_factory,
        })
    }

    pub fn process_greeting(&mut self) -> Result<&mut TestState> {
        self.stream.reregister(
            &mut self.poll,
            self.token,
            Interest::WRITABLE | Interest::READABLE,
        )?;

        debug!("Greeting process_greeting");
        self.process_phase(TestPhase::GreetingCompleted, ONE_SECOND_NS * 2)?;

        debug!("Greeting completed");

        Ok(self)
    }

    pub fn run_put_no_result(&mut self) -> Result<()> {
        self.measurement_state.phase = TestPhase::PutNoResultSendCommand;
        self.stream
            .reregister(&mut self.poll, self.token, Interest::WRITABLE)?;
        self.process_phase(TestPhase::PutNoResultCompleted, ONE_SECOND_NS * 8)?;
        Ok(())
    }

    pub fn run_perf_test(&mut self) -> Result<()> {
        self.measurement_state.phase = TestPhase::PerfNoResultSendCommand;
        self.stream
            .reregister(&mut self.poll, self.token, Interest::WRITABLE)?;
        self.process_phase(TestPhase::PerfNoResultCompleted, ONE_SECOND_NS * 12)?;
        Ok(())
    }

    pub fn run_ping(&mut self) -> Result<()> {
        self.measurement_state.phase = TestPhase::PingSendPing;
        self.stream
            .reregister(&mut self.poll, self.token, Interest::WRITABLE)?;
        self.process_phase(TestPhase::PingCompleted, ONE_SECOND_NS * 3)?;
        Ok(())
    }

    pub fn run_get_chunks(&mut self) -> Result<()> {
        debug!("Run get chunks");
        self.measurement_state.phase = TestPhase::GetChunksSendChunksCommand;
        self.stream
            .reregister(&mut self.poll, self.token, Interest::WRITABLE)?;
        self.process_phase(TestPhase::GetChunksCompleted, ONE_SECOND_NS * 3)?;
        debug!("Run get chunks completed");
        Ok(())
    }

    pub fn run_get_time(&mut self) -> Result<()> {
        self.measurement_state.phase = TestPhase::GetTimeSendCommand;
        self.stream
            .reregister(&mut self.poll, self.token, Interest::WRITABLE)?;
        self.process_phase(TestPhase::GetTimeCompleted, ONE_SECOND_NS * 12)?;
        Ok(())
    }

    pub fn run_put(&mut self) -> Result<()> {
        debug!("Run put");
        self.measurement_state.phase = TestPhase::PutSendCommand;
        self.stream
            .reregister(&mut self.poll, self.token, Interest::WRITABLE)?;
        self.process_phase(TestPhase::PutCompleted, ONE_SECOND_NS * 10)?;
        Ok(())
    }

    fn process_phase(
        &mut self,
        // mut measurement_state: MeasurementState,
        // handler_factory: &mut HandlerFactory,
        phase: TestPhase,
        test_duration_ns: u128,
    ) -> Result<()> {
        if self.measurement_state.failed {
            return Ok(());
        }

        self.measurement_state.phase_start_time = Some(Instant::now());

        while self.measurement_state.phase != phase {
            self.poll
                .poll(&mut self.events, Some(Duration::from_secs(3)))?;

            trace!("Poll iteration token {:?}", self.token);

            if (self.events.is_empty()) {
                let time = self
                    .measurement_state
                    .phase_start_time
                    .unwrap()
                    .elapsed()
                    .as_nanos();
                let now = Instant::now().elapsed().as_nanos();
                if now - time > test_duration_ns {
                    self.measurement_state.failed = true;
                    break;
                }
            }

            // Process events in the current poll iteration
            for event in &self.events {
                // Handle read events after write to process any responses
                if event.is_readable() {
                    trace!("Readable event token {:?}", self.token);
                    if let Some(handler) = self
                        .handler_factory
                        .get_handler(&self.measurement_state.phase)
                    {
                        if let Err(e) = handler.on_read(
                            &mut self.stream,
                            &self.poll,
                            &mut self.measurement_state,
                        ) {
                            debug!(
                                "Error in on_read for phase {:?}: {}",
                                self.measurement_state.phase, e
                            );
                            return Err(e);
                        }
                    }
                }
                // Handle write events first to ensure data is sent
                if event.is_writable() {
                    trace!("Writable event token {:?}", self.token);
                    if let Some(handler) = self
                        .handler_factory
                        .get_handler(&self.measurement_state.phase)
                    {
                        if let Err(e) = handler.on_write(
                            &mut self.stream,
                            &self.poll,
                            &mut self.measurement_state,
                        ) {
                            debug!(
                                "Error in on_write for phase {:?}: {}",
                                self.measurement_state.phase, e
                            );
                            return Err(e);
                        }
                    }
                }
            }
        }

        Ok(())
    }

    pub fn measurement_state(&self) -> &MeasurementState {
        &self.measurement_state
    }

    pub fn measurement_state_mut(&mut self) -> &mut MeasurementState {
        &mut self.measurement_state
    }
}
