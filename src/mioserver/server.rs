use bytes::BytesMut;
use log::debug;
use mio::net::{TcpListener, TcpStream};
use mio::{Poll, Token, Waker};
use std::collections::VecDeque;
use std::io::{self};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

use crate::config::Config;
use crate::mioserver::worker::WorkerThread;
use crate::mioserver::ServerTestPhase;
use crate::stream::stream::Stream;
use crate::tokio_server::server_config::parse_listen_address;

pub struct MioServer {
    listener: TcpListener,
    _worker_threads: Vec<WorkerThread>,
    worker_queues: Vec<Arc<(Mutex<VecDeque<TcpStream>>, Waker)>>, // Отдельная очередь для каждого воркера
    worker_connection_counts: Arc<Mutex<Vec<usize>>>, // Счетчики соединений для каждого воркера
}

pub struct TestState {
    pub token: Token,
    pub stream: Stream,
    pub measurement_state: ServerTestPhase,
    pub read_buffer: [u8; 1024 * 8],
    pub write_buffer: [u8; 1024 * 8],
    pub read_bytes: BytesMut,
    pub read_pos: usize,
    pub total_bytes: u64,
    pub write_pos: usize,
    pub num_chunks: usize,
    pub chunk_size: usize,
    pub processed_chunks: usize,
    pub clock: Option<Instant>,
    pub time_ns: Option<u128>,
    pub duration: u64,
    pub chunk_buffer: Vec<u8>,
    pub chunk: Option<BytesMut>,
    pub terminal_chunk: Option<BytesMut>,
}
impl MioServer {
    pub fn new(config: Config) -> io::Result<Self> {
        let addr: SocketAddr = parse_listen_address(&config.server_tcp_port).unwrap();
        let listener = TcpListener::bind(addr)?;

        let logical = num_cpus::get();
        let mut worker_queues = Vec::new();
        for i in 0..logical {
            let poll = Poll::new()?;
            let queue = Arc::new((
                Mutex::new(VecDeque::new()),
                Waker::new(poll.registry(), Token(i + 1))?,
            ));
            worker_queues.push(queue);
        }

        let worker_connection_counts = Arc::new(Mutex::new(vec![0; logical]));

        let mut worker_threads = Vec::new();

        for i in 0..logical {
            let worker = WorkerThread::new(
                i,
                worker_queues[i].clone(),
                worker_connection_counts.clone(),
            )?;
            worker_threads.push(worker);
        }

        debug!(
            "MIO TCP Server started on {} with {} worker threads",
            addr, logical
        );

        Ok(Self {
            listener,
            _worker_threads: worker_threads,
            worker_queues,
            worker_connection_counts,
        })
    }

    pub fn run(&mut self) -> io::Result<()> {
        loop {
            match self.listener.accept() {
                Ok((stream, _addr)) => {
                    if let Err(e) = stream.set_nodelay(true) {
                        debug!("Failed to set TCP_NODELAY: {}", e);
                    }

                    let (selected_worker, _current_counts) = {
                        let counts = self.worker_connection_counts.lock().unwrap();
                        let counts_vec: Vec<usize> = counts.clone();
                        let min_count = counts.iter().min().unwrap_or(&0);
                        let selected = counts
                            .iter()
                            .position(|&count| count == *min_count)
                            .unwrap_or(0);
                        (selected, counts_vec)
                    };

                    {
                        let mut counts = self.worker_connection_counts.lock().unwrap();
                        counts[selected_worker] += 1;
                        println!(
                            "Worker {}: connection count increased to {}",
                            selected_worker, counts[selected_worker]
                        );
                    }

                    let (lock, waker) = &*self.worker_queues[selected_worker];
                    {
                        let mut queue = lock.lock().unwrap();
                        queue.push_back(stream);
                    }
                    waker.wake()?;

                    debug!(
                        "Connection added to worker {} queue and worker woken",
                        selected_worker
                    );
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                    thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(e) => {
                    debug!("Error accepting connection: {}", e);
                    return Err(e);
                }
            }
        }
    }
}

impl Drop for MioServer {
    fn drop(&mut self) {
        debug!("MIO TCP Server shutting down");
    }
}
