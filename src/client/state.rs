use anyhow::Result;
use bytes::BytesMut;
use log::{debug, trace};
use mio::{Events, Interest, Poll, Token};
use std::collections::VecDeque;
use std::time::Instant;
use std::{net::SocketAddr, path::Path, time::Duration};
use std::io;

use crate::client::handlers::basic_handler::{
    handle_client_readable_data, handle_client_writable_data,
};
use crate::client::constants::DEFAULT_READ_BUFFER_SIZE;
use crate::config::constants::MIN_CHUNK_SIZE;
use crate::stream::stream::Stream;

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

    PerfSendCommand,
    PerfReceiveOk,
    PerfSendChunks,
    PerfSendLastChunk,
    PerfReceiveTime,
    PerfCompleted,
}

pub struct TestState {
    poll: Poll,
    events: Events,
    measurement_state: MeasurementState,
}

#[derive(Debug)]
pub struct MeasurementState {
    pub token: Token,
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
    pub read_buffer: [u8; 1024 * 8],
    pub write_buffer: [u8; 1024 * 8],
    pub read_pos: usize,
    pub write_pos: usize,
    pub measurements: VecDeque<(u64, u64)>, // Хранит (t_k^(j), b_k^(j)) для каждого чанка\
    pub upload_measurements: VecDeque<(u64, u64)>, // Хранит (t_k^(j), b_k^(j)) для каждого чанка\
    pub failed: bool,
    pub stream: Stream,
    pub total_chunks: u32,
    pub chunk_buffer: Vec<u8>,
    pub cursor: usize,
    pub ping_times: Vec<u64>, // Store all ping times for median calculation
    pub time_result: Option<u64>,
    pub bytes_received: u64,
    pub bytes_sent: u64,
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
            Stream::new_rustls(addr, cert_path, key_path)?
        } else {
            if use_websocket {
                Stream::new_websocket(addr)?
            } else {
                Stream::new_tcp(addr)?
            }
        };

        stream.register(&mut poll, token, Interest::READABLE | Interest::WRITABLE)?;

        let measurement_state = MeasurementState {
            buffer: BytesMut::with_capacity(DEFAULT_READ_BUFFER_SIZE),
            phase: TestPhase::GreetingSendConnectionType,
            upload_results_for_graph: Vec::new(),
            upload_bytes: None,
            upload_time: None,
            upload_speed: None,
            download_time: None,
            download_bytes: None,
            download_speed: None,
            chunk_size: MIN_CHUNK_SIZE as usize,
            ping_median: None,
            read_buffer: [0u8; 1024 * 8],
            measurements: VecDeque::new(),
            upload_measurements: VecDeque::new(),
            phase_start_time: None,
            failed: false,
            token,
            write_buffer: [0u8; 1024 * 8],
            read_pos: 0,
            write_pos: 0,
            stream,
            total_chunks: 1,
            chunk_buffer: Vec::with_capacity(MIN_CHUNK_SIZE as usize),
            cursor: 0,
            ping_times: Vec::new(),
            time_result: None,
            bytes_received: 0,
            bytes_sent: 0,
        };


        Ok(Self {
            poll,
            events,
            measurement_state,
        })
    }

    pub fn process_greeting(&mut self) -> Result<&mut TestState> {
        self.measurement_state.stream.reregister(
            &mut self.poll,
            self.measurement_state.token,
            Interest::WRITABLE | Interest::READABLE,
        )?;

        debug!("Greeting process_greeting");
        self.process_phase(TestPhase::GreetingCompleted, ONE_SECOND_NS * 5)?;

        debug!("Greeting completed");

        Ok(self)
    }

    pub fn run_perf_test(&mut self) -> Result<()> {
        self.measurement_state.phase = TestPhase::PerfSendCommand;
        self.measurement_state.stream.reregister(
            &mut self.poll,
            self.measurement_state.token,
            Interest::WRITABLE,
        )?;
        self.process_phase(TestPhase::PerfCompleted, ONE_SECOND_NS * 12)?;
        Ok(())
    }

    pub fn run_ping(&mut self) -> Result<()> {
        self.measurement_state.phase = TestPhase::PingSendPing;
        self.measurement_state.stream.reregister(
            &mut self.poll,
            self.measurement_state.token,
            Interest::WRITABLE,
        )?;
        self.process_phase(TestPhase::PingCompleted, ONE_SECOND_NS * 3)?;
        Ok(())
    }

    pub fn run_get_chunks(&mut self) -> Result<()> {
        debug!("Run get chunks");
        self.measurement_state.phase = TestPhase::GetChunksSendChunksCommand;
        self.measurement_state.stream.reregister(
            &mut self.poll,
            self.measurement_state.token,
            Interest::WRITABLE,
        )?;
        self.process_phase(TestPhase::GetChunksCompleted, ONE_SECOND_NS * 3)?;
        debug!("Run get chunks completed");
        Ok(())
    }

    pub fn run_get_time(&mut self) -> Result<()> {
        self.measurement_state.phase = TestPhase::GetTimeSendCommand;
        self.measurement_state.stream.reregister(
            &mut self.poll,
            self.measurement_state.token,
            Interest::WRITABLE,
        )?;
        self.process_phase(TestPhase::GetTimeCompleted, ONE_SECOND_NS * 12)?;
        Ok(())
    }

    fn process_phase(
        &mut self,
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

            if self.events.is_empty() {
                let time = self
                    .measurement_state
                    .phase_start_time
                    .unwrap()
                    .elapsed()
                    .as_nanos();
                let now = Instant::now().elapsed().as_nanos();
                if now - time > test_duration_ns {
                    debug!(
                        "Test duration exceeded {:?} for token {:?}",
                        self.measurement_state.phase, self.measurement_state.token
                    );
                    self.measurement_state.failed = true;
                    break;
                }
            }

            for event in self.events.iter() {

            // Process events in the current poll iteration
            let mut should_remove: Result<usize, io::Error> = Ok(0);

            if event.is_readable() {
                should_remove = handle_client_readable_data(&mut self.measurement_state, &self.poll);
            } else if event.is_writable() {
                should_remove = handle_client_writable_data(&mut self.measurement_state, &self.poll);
            }

                match should_remove {
                    Ok(n) => {
                        if n == 0 {
                            trace!("No data to read");
                            self.measurement_state.failed = true;
                        }
                        // If n > 0, continue processing
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        trace!("WouldBlock");
                        continue;
                    }
                    Err(e) => {
                        trace!("Error: {:?}", e);
                        self.measurement_state.failed = true;
                        break;
                    }
                }
            }
        }

        Ok(())
    }

    pub fn measurement_state(&self) -> &MeasurementState {
        &self.measurement_state
    }

}
