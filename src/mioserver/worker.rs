use bytes::BytesMut;
use log::{debug, info, trace};
use mio::{Events, Interest, Poll, Token, Waker};
use std::collections::{HashMap, VecDeque};
use std::io::{self};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::config::constants::MIN_CHUNK_SIZE;
use crate::mioserver::handlers::basic_handler::{
    handle_client_readable_data, handle_client_writable_data,
};
use crate::mioserver::server::{ConnectionType, TestState};
use crate::mioserver::ServerTestPhase;
use crate::stream::stream::Stream;

pub struct WorkerThread {
    _thread: thread::JoinHandle<()>,
}

struct Worker {
    id: usize,
    poll: Poll,
    connections: HashMap<Token, TestState>,
    events: Events,
    worker_queue: Arc<(Mutex<VecDeque<ConnectionType>>, Waker)>, // Очередь только для этого воркера
    worker_connection_counts: Arc<Mutex<Vec<usize>>>,
    server_config: crate::mioserver::server::ServerConfig,
    next_token: usize,
}

impl WorkerThread {
    pub fn new(
        id: usize,
        worker_queue: Arc<(Mutex<VecDeque<ConnectionType>>, Waker)>,
        worker_connection_counts: Arc<Mutex<Vec<usize>>>,
        server_config: crate::mioserver::server::ServerConfig,
    ) -> io::Result<Self> {
        println!("Worker {} new", id);

        let thread = thread::Builder::new()
            .stack_size(8 * 1024 * 1024) // 8MB stack
            .spawn(move || {
                println!("Worker {}: starting", id);
                let mut worker =
                    Worker::new(id, worker_queue, worker_connection_counts, server_config)
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
        worker_queue: Arc<(Mutex<VecDeque<ConnectionType>>, Waker)>,
        worker_connection_counts: Arc<Mutex<Vec<usize>>>,
        server_config: crate::mioserver::server::ServerConfig,
    ) -> io::Result<Self> {
        let poll = Poll::new()?;
        let events = Events::with_capacity(1024);
        let connections = HashMap::new();

        Ok(Worker {
            id,
            poll,
            connections,
            events,
            worker_queue,
            worker_connection_counts,
            server_config,
            next_token: 1,
        })
    }

    fn run(&mut self) -> io::Result<()> {
        loop {
            let maybe_connection = {
                let (lock, _) = &*self.worker_queue;
                let mut guard = lock.lock().unwrap();
                let queue_size = guard.len();
                if queue_size > 0 {
                    println!(
                        "Worker {}: found {} connection(s) in queue",
                        self.id, queue_size
                    );
                }
                guard.pop_front()
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
                println!("Worker {}: processing new connection", self.id);
                debug!("Worker {}: received new connection", self.id);

                let token = Token(self.next_token);
                self.next_token += 1;

                // Регистрируем новое соединение
                if let Err(e) = stream.register(&self.poll, token, Interest::READABLE) {
                    info!("Worker {}: Failed to register connection: {}", self.id, e);
                    continue;
                }

                self.connections.insert(
                    token,
                    TestState {
                        token,
                        // stream: Stream::new_rustls_server(stream, None, None).unwrap(),
                        stream: stream,
                        measurement_state: ServerTestPhase::GreetingReceiveConnectionType,
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
                    },
                );

                // Счетчик уже увеличен при назначении соединения этому воркеру
                println!(
                    "Worker {}: processing assigned connection (count already updated)",
                    self.id
                );

                debug!(
                    "Worker {} registered new connection with token {:?} (total connections: {})",
                    self.id,
                    token,
                    self.connections.len()
                );
            }

            // Обрабатываем все активные соединения
            if !self.connections.is_empty() {
                self.process_all_connections()?;
            } else {
                // Нет соединений, спим
                thread::sleep(Duration::from_millis(50));
            }
        }
    }

    fn process_all_connections(&mut self) -> io::Result<()> {
        if let Err(e) = self.poll.poll(
            &mut self.events,
            Some(std::time::Duration::from_millis(100)),
        ) {
            info!("Worker {}: Poll error: {}", self.id, e);
            return Err(e);
        }

        let mut connections_to_remove = Vec::new();

        for event in self.events.iter() {
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
                        // If n > 0, continue processing
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        continue;
                    }
                    Err(e) => {
                        info!(
                            "Worker {}: Error handling client data for token {:?} with error {:?}",
                            self.id, event_token, e
                        );
                        connections_to_remove.push(event_token);
                    }
                }
            }
        }

        for token in connections_to_remove {
            self.connections.remove(&token);
            {
                let mut counts = self.worker_connection_counts.lock().unwrap();
                counts[self.id] -= 1;
            }

            debug!(
                "Worker {}: connection {:?} closed, remaining connections: {}",
                self.id,
                token,
                self.connections.len()
            );
        }

        Ok(())
    }
}
