use bytes::BytesMut;
use mio::{Events, Interest, Poll, Token, Waker};
use mio::net::{TcpListener, TcpStream};
use std::collections::{HashMap, VecDeque};
use std::io::{self};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Instant, Duration};
use log::{debug, info, trace};

use crate::client::Stream;
use crate::config::constants::MIN_CHUNK_SIZE;
use crate::mioserver::handlers::basic_handler::{handle_client_readable_data, handle_client_writable_data};
use crate::mioserver::handlers::greeting_handler::{handle_greeting_accep_token_read, handle_greeting_send_accept_token};
use crate::mioserver::ServerTestPhase;

const MAX_CONNECTIONS: usize = 1024;

pub struct WorkerThread {
    thread: thread::JoinHandle<()>,
}

pub struct MioServer {
    listener: TcpListener,
    worker_threads: Vec<WorkerThread>,
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
    pub write_pos: usize,
    pub num_chunks: usize,
    pub chunk_size: usize,
    pub processed_chunks: usize,
    pub clock: Option<Instant>,
    pub time_ns: Option<u128>,
    pub duration: u64,
    pub chunk_buffer: Vec<u8>,
}

impl WorkerThread {
    fn new(id: usize, worker_queue: Arc<(Mutex<VecDeque<TcpStream>>, Waker)>, worker_connection_counts: Arc<Mutex<Vec<usize>>>) -> io::Result<Self> {
        println!("Worker {} new", id);

        let thread = thread::Builder::new()
            .stack_size(8 * 1024 * 1024) // 8MB stack
            .spawn(move || {
                println!("Worker {}: starting", id);
                let mut worker = Worker::new(id, worker_queue, worker_connection_counts).expect("Failed to create worker");
                if let Err(e) = worker.run() {
                    info!("Worker {} error: {}", id, e);
                }
            })?;
        
        Ok(WorkerThread { thread })
    }
}

struct Worker {
    id: usize,
    poll: Poll,
    connections: HashMap<Token, TestState>,
    events: Events,
    worker_queue: Arc<(Mutex<VecDeque<TcpStream>>, Waker)>, // Очередь только для этого воркера
    worker_connection_counts: Arc<Mutex<Vec<usize>>>,
    next_token: usize,
}

impl Worker {
    fn new(id: usize, worker_queue: Arc<(Mutex<VecDeque<TcpStream>>, Waker)>, worker_connection_counts: Arc<Mutex<Vec<usize>>>) -> io::Result<Self> {
        println!("Worker {}: creating Poll", id);
        let poll = Poll::new()?;
        println!("Worker {}: Poll created, creating Events", id);
        let events = Events::with_capacity(MAX_CONNECTIONS);
        println!("Worker {}: Events created, creating HashMap", id);
        let connections = HashMap::new();
        println!("Worker {}: HashMap created, creating Worker struct", id);
        
        Ok(Worker {
            id,
            poll,
            connections,
            events,
            worker_queue,
            worker_connection_counts,
            next_token: 1,
        })
    }
    
    fn run(&mut self) -> io::Result<()> {
        println!("Worker {}: entering run method", self.id);
        
        loop {
            // Проверяем, есть ли новые соединения в очереди этого воркера
            let maybe_stream = {
                let (lock, _) = &*self.worker_queue;
                let mut guard = lock.lock().unwrap();
                let queue_size = guard.len();
                if queue_size > 0 {
                    println!("Worker {}: found {} connection(s) in queue", self.id, queue_size);
                }
                guard.pop_front()
            };

            if let Some(mut stream) = maybe_stream {
                println!("Worker {}: processing new connection", self.id);
                debug!("Worker {}: received new connection", self.id);
                
                let token = Token(self.next_token);
                self.next_token += 1;
                
                // Регистрируем новое соединение
                if let Err(e) = self.poll.registry().register(
                    &mut stream,
                    token,
                    Interest::READABLE,
                ) {
                    info!("Worker {}: Failed to register connection: {}", self.id, e);
                    continue;
                }
                
                self.connections.insert(token, TestState {
                    token,
                    stream: Stream::Tcp(stream),
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
                });
                
                // Счетчик уже увеличен при назначении соединения этому воркеру
                println!("Worker {}: processing assigned connection (count already updated)", self.id);
                
                debug!("Worker {} registered new connection with token {:?} (total connections: {})", 
                       self.id, token, self.connections.len());
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
        // Обрабатываем события с таймаутом
        if let Err(e) = self.poll.poll(&mut self.events, Some(std::time::Duration::from_millis(100))) {
            info!("Worker {}: Poll error: {}", self.id, e);
            return Err(e);
        }
        
        let mut connections_to_remove = Vec::new();
        
        for event in self.events.iter() {
            let event_token = event.token();
            
            if let Some(state) = self.connections.get_mut(&event_token) {
                let mut should_remove = false;
                
                if event.is_readable() {
                    trace!("Worker {}: event is readable for token {:?}", self.id, event_token);
                    should_remove = handle_client_readable_data(state, &self.poll).is_err();
                }
                if event.is_writable() {
                    trace!("Worker {}: event is writable for token {:?}", self.id, event_token);
                    should_remove = handle_client_writable_data(state, &self.poll).is_err();
                }
                
                if should_remove {
                    info!("Worker {}: Error handling client data for token {:?}", self.id, event_token);
                    connections_to_remove.push(event_token);
                }
            }
        }
        
        // Удаляем завершенные соединения
        for token in connections_to_remove {
            self.connections.remove(&token);
            
            // Уменьшаем счетчик соединений для этого воркера
            {
                let mut counts = self.worker_connection_counts.lock().unwrap();
                counts[self.id] -= 1;
            }
            
            debug!("Worker {}: connection {:?} closed, remaining connections: {}", 
                   self.id, token, self.connections.len());
        }
        
        Ok(())
    }
}

impl MioServer {
    pub fn new(addr: SocketAddr) -> io::Result<Self> {
        let listener = TcpListener::bind(addr)?;
        
        let logical = num_cpus::get();
        println!("Logical: {}", logical);
        
        // Создаем отдельную очередь и Waker для каждого воркера
        let mut worker_queues = Vec::new();
        for i in 0..logical {
            let poll = Poll::new()?;
            let queue = Arc::new((
                Mutex::new(VecDeque::new()),
                Waker::new(poll.registry(), Token(i + 1))?
            ));
            worker_queues.push(queue);
        }
        
        // Создаем счетчики соединений для каждого воркера
        let worker_connection_counts = Arc::new(Mutex::new(vec![0; logical]));
        
        // Создаем воркеры
        let mut worker_threads = Vec::new();
        
        for i in 0..logical {
            let worker = WorkerThread::new(i, worker_queues[i].clone(), worker_connection_counts.clone())?;
            worker_threads.push(worker);
        }
        
        debug!("MIO TCP Server started on {} with {} worker threads", addr, logical);
        
        Ok(Self {
            listener,
            worker_threads,
            worker_queues,
            worker_connection_counts,
        })
    }

    pub fn run(&mut self) -> io::Result<()> {
        loop {
            match self.listener.accept() {
                Ok((mut stream, addr)) => {
                    // Настраиваем поток для неблокирующего режима
                    if let Err(e) = stream.set_nodelay(true) {
                        debug!("Failed to set TCP_NODELAY: {}", e);
                    }
                    
                    // Выбираем воркера с наименьшим количеством соединений
                    let (selected_worker, current_counts) = {
                        let counts = self.worker_connection_counts.lock().unwrap();
                        let counts_vec: Vec<usize> = counts.clone();
                        let min_count = counts.iter().min().unwrap_or(&0);
                        let selected = counts.iter().position(|&count| count == *min_count).unwrap_or(0);
                        (selected, counts_vec)
                    };
                    
                    println!("Available workers: {:?}", current_counts);
                    println!("Selected worker {} for new connection (connection count: {})", 
                           selected_worker, current_counts[selected_worker]);
                    
                    // Увеличиваем счетчик соединений для выбранного воркера СРАЗУ
                    {
                        let mut counts = self.worker_connection_counts.lock().unwrap();
                        counts[selected_worker] += 1;
                        println!("Worker {}: connection count increased to {}", selected_worker, counts[selected_worker]);
                    }
                    
                    // Кладим соединение в очередь выбранного воркера и будим его
                    let (lock, waker) = &*self.worker_queues[selected_worker];
                    {
                        let mut queue = lock.lock().unwrap();
                        queue.push_back(stream);
                    }
                    waker.wake()?;
                    
                    debug!("Connection added to worker {} queue and worker woken", selected_worker);
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                    // Нет новых соединений
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