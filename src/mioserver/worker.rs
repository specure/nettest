use bytes::BytesMut;
use log::{debug, info, trace};
use mio::{Events, Interest, Poll, Token};
use regex::Regex;
use std::collections::{HashMap, VecDeque};
use std::io::{self};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::config::constants::MIN_CHUNK_SIZE;
use crate::mioserver::handlers::basic_handler::{
    handle_client_readable_data, handle_client_writable_data,
};
use crate::mioserver::server::{ConnectionType, ServerConfig, TestState};
use crate::mioserver::ServerTestPhase;
use crate::stream::stream::Stream;
use crate::tokio_server::utils::use_http::RMBT_UPGRADE;
use crate::tokio_server::utils::websocket::Handshake;

pub struct WorkerThread {
    _thread: thread::JoinHandle<()>,
}

struct Worker {
    id: usize,
    poll: Poll,
    connections: HashMap<Token, TestState>,
    events: Events,
    worker_connection_counts: Arc<Mutex<Vec<usize>>>,
    global_queue: Arc<Mutex<VecDeque<(ConnectionType, Instant)>>>, // Общая очередь
    server_config: ServerConfig,
    next_token: usize,
    
}

impl WorkerThread {
    pub fn new(
        id: usize,
        worker_connection_counts: Arc<Mutex<Vec<usize>>>,
        global_queue: Arc<Mutex<VecDeque<(ConnectionType, Instant)>>>,
        server_config: ServerConfig,
    ) -> io::Result<Self> {

        let thread = thread::Builder::new()
            .stack_size(8 * 1024 * 1024) // 8MB stack
            .spawn(move || {
                debug!("Worker {}: starting", id);
                let mut worker =
                    Worker::new(id, worker_connection_counts, global_queue, server_config)
                        .expect("Failed to create worker");
                if let Err(e) = worker.run() {
                    info!("Worker {} error: {}", id, e);
                }
            })?;

        Ok(WorkerThread { _thread: thread })
    }
}

impl Worker {
    fn new(
        id: usize,
        worker_connection_counts: Arc<Mutex<Vec<usize>>>,
        global_queue: Arc<Mutex<VecDeque<(ConnectionType, Instant)>>>,
        server_config: ServerConfig,
    ) -> io::Result<Self> {
        let poll = Poll::new()?;
        let events = Events::with_capacity(1024);
        let connections = HashMap::new();

        Ok(Worker {
            id,
            poll,
            connections,
            events,
            worker_connection_counts,
            global_queue,
            server_config: server_config.clone(),
            next_token: 1,
        })
    }

    fn run(&mut self) -> io::Result<()> {
        loop {
            trace!("Worker {}: loop iteration, connections: {}, queue size: {}", 
                self.id, 
                self.connections.len(),
                self.global_queue.lock().unwrap().len()
            );
            let maybe_connection = if self.connections.is_empty() {
                let mut global_queue = self.global_queue.lock().unwrap();
                if let Some((connection, _)) = global_queue.pop_front() {
                    trace!("Worker {}: taking connection from global queue (queue size after: {})", 
                        self.id, global_queue.len());
                    {
                        let mut counts = self.worker_connection_counts.lock().unwrap();
                        counts[self.id] += 1;
                        trace!(
                            "Worker {}: connection count increased to {} (from global queue)",
                            self.id,
                            counts[self.id]
                        );
                    }
                    Some(connection)
                } else {
                    None
                }
            } else {
                None
            };

            if let Some(connection) = maybe_connection {
                let mut stream = match connection {
                    ConnectionType::Tcp(stream) => Stream::Tcp(stream),
                    ConnectionType::Tls(stream) => {
                        let stream = Stream::new_rustls_server(
                            stream,
                            self.server_config.cert_path.clone().unwrap(),
                            self.server_config.key_path.clone().unwrap(),
                        )
                        .unwrap();
                        stream
                    }
                };

                let token = Token(self.next_token);
                self.next_token += 1;

                // Регистрируем новое соединение
                if let Err(e) = stream.register(&self.poll, token, Interest::READABLE) {
                    info!("Worker {}: Failed to register connection: {}", self.id, e);
                    continue;
                }

                stream = self.handle_greeting_receive_connection_type(stream, token)?;

                self.connections.insert(
                    token,
                    TestState {
                        token,
                        // stream: Stream::new_rustls_server(stream, None, None).unwrap(),
                        stream: stream,
                        measurement_state: ServerTestPhase::GreetingSendVersion,
                        read_buffer: [0; 1024 * 8],
                        write_buffer: [0; 1024 * 8],
                        read_bytes: BytesMut::new(),
                        read_pos: 0,
                        write_pos: 0,
                        num_chunks: 0,
                        chunk_size: 0,
                        processed_chunks: 0,
                        clock: None,
                        time_ns: None,
                        duration: 0,
                        chunk_buffer: vec![0; MIN_CHUNK_SIZE as usize],
                        total_bytes: 0,
                        chunk: None,
                        terminal_chunk: None,
                        put_duration: None,
                        bytes_received: VecDeque::new(),
                    },
                );

                debug!(
                    "Worker {} registered new connection with token {:?} (total connections: {})",
                    self.id,
                    token,
                    self.connections.len()
                );
            }

            if !self.connections.is_empty() {
                self.process_all_connections()?;
            } else {
                thread::sleep(Duration::from_millis(100));
            }
        }
    }

    fn process_all_connections(&mut self) -> io::Result<()> {
        if let Err(e) = self
            .poll
            .poll(&mut self.events, Some(std::time::Duration::from_millis(10)))
        {
            info!("Worker {}: Poll error: {}", self.id, e);
            return Err(e);
        }

        let mut connections_to_remove = Vec::new();

        for event in self.events.iter() {
            debug!("Worker {}: event {:?} token {:?}", self.id, event, event.token());
            let event_token = event.token();

            if let Some(state) = self.connections.get_mut(&event_token) {
                let mut should_remove: Result<usize, io::Error> = Ok(0);

                if event.is_readable() {
                    trace!(
                        "Worker {}: event is readable for token {:?}",
                        self.id,
                        event_token
                    );
                    should_remove = handle_client_readable_data(state, &self.poll);
                } else if event.is_writable() {
                    trace!(
                        "Worker {}: event is writable for token {:?}",
                        self.id,
                        event_token
                    );
                    should_remove = handle_client_writable_data(state, &self.poll);
                }

                match should_remove {
                    Ok(n) => {
                        if n == 0 {
                            connections_to_remove.push(event_token);
                        }
                        debug!("Worker {}: should_remove: {} token {:?}", self.id, n, event_token);
                        continue;
                        // If n > 0, continue processing
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        debug!("Worker {}: would block for token {:?}", self.id, event_token);
                        continue;
                    }
                    Err(e) => {
                        info!(
                            "Worker {}: Error handling client data for token {:?} with error {:?} and measurement state {:?}",
                            self.id, event_token, e, state.measurement_state
                        );
                        connections_to_remove.push(event_token);
                    }
                }
            }
        }



        for token in connections_to_remove {
            self.connections.remove(&token);
            {
                println!("Worker {}: removing connection {:?}", self.id, token);
                let mut counts = self.worker_connection_counts.lock().unwrap();
                counts[self.id] -= 1;
                println!("Worker {}: connection count decreased to {}", self.id, counts[self.id]);
            }

            println!(
                "Worker {}: connection {:?} closed, remaining connections: {}",
                self.id,
                token,
                self.connections.len()
            );
        }

        debug!("Worker {}: finished processing events", self.id);

        Ok(())
    }

    fn handle_greeting_receive_connection_type(
        &mut self,
        mut stream: Stream,
        token: Token,
    ) -> io::Result<Stream> {
        debug!("Worker {}: handle_greeting_receive_connection_type", self.id);
        let mut buffer = vec![0; 1024];
        let mut result = BytesMut::new();
        let mut loop_flag = false;

        while !loop_flag {
            self.poll.poll(&mut self.events, None)?;
            for event in self.events.iter() {
                if event.is_readable() {
                    match stream.read(&mut buffer) {
                        Ok(n) => {
                            result.extend_from_slice(&buffer[..n]);
                            if result.len() >= 4
                                && result[result.len() - 4..result.len()]
                                    == [b'\r', b'\n', b'\r', b'\n']
                            {
                                let request = String::from_utf8_lossy(&result);
                                let ws_regex = Regex::new(r"(?i)upgrade:\s*websocket").unwrap();

                                let is_websocket = ws_regex.is_match(&request);
                                debug!("Worker {}: is_websocket: {}", self.id, is_websocket);
                                if is_websocket {
                                    stream = stream.upgrade_to_websocket().unwrap();
                                    let handshake = Handshake::parse(&request).unwrap();
                                    stream.finish_server_handshake(handshake).unwrap();
                                } else {
                                    //TODO maybe loop
                                    debug!("Worker {}: writing upgrade response", self.id);
                                    match stream.write(RMBT_UPGRADE.as_bytes()) {
                                        Ok(n) => {
                                            debug!("Worker {}: wrote {} bytes", self.id, n);
                                        }
                                        Err(e) => {
                                            debug!("Worker {}: error writing upgrade response: {}", self.id, e);
                                        }
                                    }
                                }

                                debug!("Worker {}: reregistering stream", self.id);
                                stream.reregister(&self.poll, token, Interest::WRITABLE)?;

                                loop_flag = true;
                            }
                        }
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                            continue;
                        }
                        Err(e) => {
                            return Err(e);
                        }
                    }
                }
            }
        }
        Ok(stream)
    }
}
