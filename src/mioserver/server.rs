use bytes::BytesMut;
use log::{debug, info, LevelFilter};
use mio::net::{TcpListener, TcpStream};
use mio::{Poll, Token, Waker};
use std::collections::VecDeque;
use std::io::{self};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

#[derive(Debug)]
pub enum ConnectionType {
    Tcp(TcpStream),
    Tls(TcpStream), // Пока что тот же TcpStream, но с флагом TLS
}

use crate::config::FileConfig;
use crate::mioserver::worker::WorkerThread;
use crate::mioserver::ServerTestPhase;
use crate::stream::stream::Stream;
use crate::tokio_server::server_config::parse_listen_address;

pub struct MioServer {
    tcp_listener: TcpListener,
    tls_listener: Option<TcpListener>,
    _worker_threads: Vec<WorkerThread>,
    global_queue: Arc<Mutex<VecDeque<(ConnectionType, Instant)>>>, // Общая очередь с временными метками
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
    pub put_duration: Option<u128>,
    pub chunk_buffer: Vec<u8>,
    pub chunk: Option<BytesMut>,
    pub terminal_chunk: Option<BytesMut>,
    pub bytes_received: VecDeque<(u64, u64)>
}

#[derive(Clone)]
pub struct ServerConfig {
    pub tcp_address: SocketAddr,
    pub tls_address: SocketAddr,
    pub cert_path: Option<String>,
    pub key_path: Option<String>,
    pub num_workers: Option<usize>,
    pub user: Option<String>,
    pub daemon: bool,
    pub version: Option<String>,
    pub secret_key: Option<String>,
    pub log_level: Option<LevelFilter>,
}

impl MioServer {
    pub fn new(args: Vec<String>, config: FileConfig) -> io::Result<Self> {
        let server_config = crate::mioserver::parser::parse_args(args, config.clone())
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        let tcp_listener = TcpListener::bind(server_config.tcp_address)?;

        let tls_listener = if server_config.cert_path.is_some() && server_config.key_path.is_some() {
            let tls_addr: SocketAddr = parse_listen_address(&server_config.tls_address.to_string()).unwrap();
            match TcpListener::bind(tls_addr) {
                Ok(listener) => {
                    debug!("MIO TLS Server will listen on {}", tls_addr);
                    Some(listener)
                }
                Err(e) => {
                    debug!("Failed to bind TLS listener: {}", e);
                    None
                }
            }
        } else {
            None
        };

        let logical = server_config.num_workers.unwrap_or(30);
        let mut worker_queues = Vec::new();
        for i in 0..logical {
            let poll = Poll::new()?;
            let queue = Arc::new((
                Mutex::new(VecDeque::<ConnectionType>::new()),
                Waker::new(poll.registry(), Token(i + 1))?,
            ));
            worker_queues.push(queue);
        }

        let worker_connection_counts = Arc::new(Mutex::new(vec![0; logical]));
        let global_queue = Arc::new(Mutex::new(VecDeque::new()));

        let mut worker_threads = Vec::new();

        for i in 0..logical {
            let worker = WorkerThread::new(
                i,
                worker_connection_counts.clone(),
                global_queue.clone(),
                server_config.clone(),
            )?;
            worker_threads.push(worker);
        }


        Ok(Self {
            tcp_listener,
            tls_listener,
            _worker_threads: worker_threads,
            global_queue,
        })
    }

    pub fn run(&mut self) -> io::Result<()> {
        loop {
            // Принимаем TCP соединения
            match self.tcp_listener.accept() {
                Ok((stream, _addr)) => {
                    if let Err(e) = stream.set_nodelay(true) {
                        debug!("Failed to set TCP_NODELAY: {}", e);
                    }
                    self.handle_connection(stream, false)?;
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                    // Продолжаем
                }
                Err(e) => {
                    debug!("Error accepting TCP connection: {}", e);
                    return Err(e);
                }
            }

            // Принимаем TLS соединения если есть listener
            if let Some(ref mut tls_listener) = self.tls_listener {
                match tls_listener.accept() {
                    Ok((stream, _addr)) => {
                        if let Err(e) = stream.set_nodelay(true) {
                            debug!("Failed to set TCP_NODELAY: {}", e);
                        }
                        self.handle_connection(stream, true)?;
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                        // Продолжаем
                    }
                    Err(e) => {
                        debug!("Error accepting TLS connection: {}", e);
                        return Err(e);
                    }
                }
            }

            // Проверяем общую очередь на устаревшие соединения
            self.check_global_queue()?;

            thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    fn check_global_queue(&mut self) -> io::Result<()> {
        // Убираем таймаут - соединения будут ждать пока воркер их не заберет
        Ok(())
    }

    fn handle_connection(&mut self, stream: TcpStream, is_tls: bool) -> io::Result<()> {
        let connection = if is_tls {
            ConnectionType::Tls(stream)
        } else {
            ConnectionType::Tcp(stream)
        };

        // Добавляем соединение в глобальную очередь
        let mut global_queue = self.global_queue.lock().unwrap();
        global_queue.push_back((connection, Instant::now()));
        println!(
            "SERVER: {} connection added to global queue (queue size: {})",
            if is_tls { "TLS" } else { "TCP" },
            global_queue.len()
        );
        info!(
            "{} connection added to global queue (queue size: {})",
            if is_tls { "TLS" } else { "TCP" },
            global_queue.len()
        );

        Ok(())
    }
}

impl Drop for MioServer {
    fn drop(&mut self) {
        debug!("MIO TCP Server shutting down");
    }
}
