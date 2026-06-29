use anyhow::Result;
use log::{debug, info, trace};
use mio::{Events, Interest, Poll, Token};
use std::collections::VecDeque;
use std::time::Instant;
use std::{net::SocketAddr, path::Path, time::Duration};
use std::io;

use crate::client::handlers::basic_handler::{
    handle_client_readable_data, handle_client_writable_data,
};
use crate::client::constants::{MIN_CHUNK_SIZE};
use crate::client::live::LiveSink;
use crate::stream::stream::Stream;
use crate::voip::{RtpQoSResult, VoipParams};
use crate::udp::UdpQoSResult;

pub const ONE_SECOND_NS: u128 = 1_000_000_000;

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

    PutSendCommand,
    PutReceiveOk,
    PutSendChunks,
    PutReceiveTimeBytes,
    PutSendLastChunk,
    PutReceiveFinalTime,
    PutCompleted,

    SignedResultSend,
    SignedResultReceive,
    SignedResultSendOk,
    SignedResultCompleted,

    VoipSendCommand,
    VoipReceiveOk,
    VoipSendGetResult,
    VoipReceiveResult,
    VoipCompleted,

    UdpSendTestOut,
    UdpReceiveOkOut,
    UdpSendGetResultOut,
    UdpReceiveResultOut,
    UdpSendTestIn,
    UdpSendGetResultIn,
    UdpReceiveResultIn,
    UdpCompleted,
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
    pub upload_bytes: Option<u64>,
    pub upload_time: Option<u64>,
    pub upload_speed: Option<f64>,
    pub download_time: Option<u64>,
    pub chunk_size: usize,
    pub ping_median: Option<u64>,
    pub phase_start_time: Option<Instant>,
    pub read_buffer: [u8; 1024 * 8 * 16],
    pub write_buffer: [u8; 1024 * 8 * 16],
    pub read_pos: usize,
    pub write_pos: usize,
    pub download_measurements: VecDeque<(u64, u64)>, // Stores (t_k^(j), b_k^(j)) for each chunk
    pub upload_measurements: VecDeque<(u64, u64)>, // Stores (t_k^(j), b_k^(j)) for each chunk
    pub failed: bool,
    pub stream: Stream,
    pub total_chunks: u32,
    pub chunk_buffer: Vec<u8>,
    pub cursor: usize,
    pub ping_times: Vec<u64>, // Store all ping times for median calculation
    pub time_result: Option<u64>,
    pub bytes_received: u64,
    pub bytes_sent: u64,
    pub time_result_buffer: Vec<u8>,
    pub envelope: Option<String>,
    pub server_addr: std::net::SocketAddr,
    pub voip_ssrc: Option<u32>,
    pub voip_params: Option<VoipParams>,
    pub voip_result_in: Option<RtpQoSResult>,
    pub voip_result_out: Option<RtpQoSResult>,
    pub server_udp_port: u16,
    pub udp_out_port: Option<u16>,
    pub udp_out_uuid: Option<[u8; 16]>,
    pub udp_in_uuid: Option<[u8; 16]>,
    pub udp_in_port: Option<u16>,
    pub udp_in_socket: Option<std::net::UdpSocket>,
    pub udp_result_out: Option<UdpQoSResult>,
    pub udp_result_in: Option<UdpQoSResult>,
    pub udp_server_received_out: Option<u32>,
    /// Optional sink for publishing live per-thread samples during a phase.
    pub live_sink: Option<LiveSink>,
    /// Throttle marker for live publishing.
    pub last_live_publish: Option<Instant>,
    /// PUTTIMERESULT interim reporting interval in ms (0 = only final result).
    pub puttimeresult_interval_ms: u64,
    /// Per-phase durations in ms (configurable from the client).
    pub download_duration_ms: u64,
    pub upload_duration_ms: u64,
    pub jitter_duration_ms: u64,
    pub packetloss_duration_ms: u64,
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
            debug!("Creating WebSocket TLS stream");
            let stream = Stream::new_websocket_tls(addr)?;
            debug!("WebSocket TLS stream created");
            stream
        } else if use_tls {
            debug!("Creating Rustls stream {:?}", addr);
            Stream::new_rustls(addr, cert_path, key_path)?
        } else {
            if use_websocket {
                debug!("Creating WebSocket stream");
                Stream::new_websocket(addr)?
            } else {
                Stream::new_tcp(addr)?
            }
        };

        debug!("Registering stream");
        stream.register(&mut poll, token, Interest::READABLE | Interest::WRITABLE)?;
        debug!("Stream registered");

        let measurement_state = MeasurementState {
            phase: TestPhase::GreetingSendConnectionType,
            upload_bytes: None,
            upload_time: None,
            upload_speed: None,
            download_time: None,
            chunk_size: MIN_CHUNK_SIZE as usize,
            ping_median: None,
            read_buffer: [0u8; 1024 * 8 * 16],
            download_measurements: VecDeque::new(),
            upload_measurements: VecDeque::new(),
            phase_start_time: None,
            failed: false,
            token,
            write_buffer: [0u8; 1024 * 8 * 16],
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
            time_result_buffer: Vec::new(),
            envelope: None,
            server_addr: addr,
            voip_ssrc: None,
            voip_params: None,
            voip_result_in: None,
            voip_result_out: None,
            server_udp_port: crate::udp::DEFAULT_UDP_SERVER_PORT,
            udp_out_port: None,
            udp_out_uuid: None,
            udp_in_uuid: None,
            udp_in_port: None,
            udp_in_socket: None,
            udp_result_out: None,
            udp_result_in: None,
            udp_server_received_out: None,
            live_sink: None,
            last_live_publish: None,
            puttimeresult_interval_ms: 0,
            download_duration_ms: 7000,
            upload_duration_ms: 7000,
            jitter_duration_ms: 5000,
            packetloss_duration_ms: 5000,
        };


        Ok(Self {
            poll,
            events,
            measurement_state,
        })
    }

    /// Attach a live sample sink so this thread publishes its samples while a
    /// phase is running.
    pub fn set_live_sink(&mut self, sink: LiveSink) {
        self.measurement_state.live_sink = Some(sink);
    }

    /// Set the PUTTIMERESULT interim reporting interval (ms; 0 = final only).
    pub fn set_puttimeresult_interval(&mut self, ms: u64) {
        self.measurement_state.puttimeresult_interval_ms = ms;
    }

    /// Set the per-phase durations (ms) from the client config.
    pub fn set_durations(&mut self, download_ms: u64, upload_ms: u64, jitter_ms: u64, packetloss_ms: u64) {
        self.measurement_state.download_duration_ms = download_ms;
        self.measurement_state.upload_duration_ms = upload_ms;
        self.measurement_state.jitter_duration_ms = jitter_ms;
        self.measurement_state.packetloss_duration_ms = packetloss_ms;
    }

    /// Publish the current download/upload samples to the live sink so the
    /// polling UI can redraw the graph during a phase. Throttled to ~100 ms
    /// unless `force` is set (used to flush the final snapshot of a phase).
    fn publish_live(&mut self, force: bool) {
        let (dl_sink, ul_sink) = match &self.measurement_state.live_sink {
            Some(s) => (s.download.clone(), s.upload.clone()),
            None => return,
        };
        let now = Instant::now();
        let due = force
            || match self.measurement_state.last_live_publish {
                Some(t) => now.duration_since(t) >= Duration::from_millis(100),
                None => true,
            };
        if !due {
            return;
        }
        if let Ok(mut g) = dl_sink.lock() {
            g.clear();
            g.extend(self.measurement_state.download_measurements.iter().cloned());
        }
        if let Ok(mut g) = ul_sink.lock() {
            g.clear();
            g.extend(self.measurement_state.upload_measurements.iter().cloned());
        }
        self.measurement_state.last_live_publish = Some(now);
    }

    pub fn process_greeting(&mut self) -> Result<&mut TestState> {
        self.measurement_state.stream.reregister(
            &mut self.poll,
            self.measurement_state.token,
            Interest::WRITABLE | Interest::READABLE,
        )?;

        debug!("Greeting process_greeting");
        self.process_phase(TestPhase::GreetingCompleted, ONE_SECOND_NS * 50)?;

        debug!("Greeting completed");

        Ok(self)
    }

    pub fn reset_failed(&mut self) {
        self.measurement_state.failed = false;
    }

    pub fn run_signed_result(&mut self) -> Result<()> {
        self.measurement_state.phase = TestPhase::SignedResultSend;
        self.measurement_state.stream.reregister(
            &mut self.poll,
            self.measurement_state.token,
            Interest::WRITABLE,
        )?;
        self.process_phase(TestPhase::SignedResultCompleted, ONE_SECOND_NS * 12)?;
        Ok(())
    }

    pub fn run_perf_test(&mut self) -> Result<()> {
        self.measurement_state.phase = TestPhase::PerfSendCommand;
        self.measurement_state.stream.reregister(
            &mut self.poll,
            self.measurement_state.token,
            Interest::WRITABLE,
        )?;
        // upload duration + buffer for command/result round-trips
        let perf_timeout = self.measurement_state.upload_duration_ms as u128 * 1_000_000 + ONE_SECOND_NS * 5;
        self.process_phase(TestPhase::PerfCompleted, perf_timeout)?;
        Ok(())
    }

    pub fn run_udp_test(&mut self) -> Result<()> {
        self.measurement_state.phase = TestPhase::UdpSendTestOut;
        self.measurement_state.stream.reregister(
            &mut self.poll,
            self.measurement_state.token,
            Interest::WRITABLE,
        )?;
        // OUT(duration) + tmax(3s) + IN(duration) + tmax(3s) + buffer
        let dur = self.measurement_state.packetloss_duration_ms as u128 * 1_000_000;
        let udp_timeout = 2 * (dur + ONE_SECOND_NS * 3) + ONE_SECOND_NS * 3;
        self.process_phase(TestPhase::UdpCompleted, udp_timeout)?;
        Ok(())
    }

    pub fn run_voip_test(&mut self) -> Result<()> {
        self.measurement_state.phase = TestPhase::VoipSendCommand;
        self.measurement_state.stream.reregister(
            &mut self.poll,
            self.measurement_state.token,
            Interest::WRITABLE,
        )?;
        // jitter duration + buffer
        let voip_timeout = self.measurement_state.jitter_duration_ms as u128 * 1_000_000 + ONE_SECOND_NS * 5;
        self.process_phase(TestPhase::VoipCompleted, voip_timeout)?;
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
        let get_time_timeout = self.measurement_state.download_duration_ms as u128 * 1_000_000 + ONE_SECOND_NS * 5;
        self.process_phase(TestPhase::GetTimeCompleted, get_time_timeout)?;
        Ok(())
    }

    pub fn run_put(&mut self) -> Result<()> {
        self.measurement_state.phase = TestPhase::PutSendCommand;
        self.measurement_state.stream.reregister(
            &mut self.poll,
            self.measurement_state.token,
            Interest::WRITABLE,
        )?;
        let put_timeout = self.measurement_state.upload_duration_ms as u128 * 1_000_000 + ONE_SECOND_NS * 5;
        self.process_phase(TestPhase::PutCompleted, put_timeout)?;
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
                .poll(&mut self.events, Some(Duration::from_nanos(test_duration_ns as u64)))?;

            if self.events.is_empty() {
                let elapsed = self
                    .measurement_state
                    .phase_start_time
                    .unwrap()
                    .elapsed()
                    .as_nanos();
                if elapsed > test_duration_ns {
                    info!(
                        "Test duration exceeded {:?} for token {:?}",
                        self.measurement_state.phase, self.measurement_state.token
                    );
                    self.measurement_state.failed = true;
                    return Err(anyhow::anyhow!("Test duration exceeded"));
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
                            info!("No data to read for token {:?} phase: {:?}", self.measurement_state.token, self.measurement_state.phase);
                            self.measurement_state.failed = true;
                            return Ok(());
                        }
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        trace!("WouldBlock");
                        continue;
                    }
                    Err(e) => {
                        info!("Error: {:?} for token {:?} phase: {:?}", e, self.measurement_state.token, self.measurement_state.phase);
                        self.measurement_state.failed = true;
                        return Ok(());
                    }
                }
            }

            // Publish a live snapshot for the polling UI (throttled).
            self.publish_live(false);
        }

        // Flush the final snapshot of this phase (e.g. upload results that
        // arrive in one burst right before the phase completes).
        self.publish_live(true);

        Ok(())
    }

    pub fn set_udp_port(&mut self, port: u16) {
        self.measurement_state.server_udp_port = port;
    }

    pub fn measurement_state(&self) -> &MeasurementState {
        &self.measurement_state
    }

}
