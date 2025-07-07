use bytes::BytesMut;
use mio::{Events, Interest, Poll, Token};
use mio::net::{TcpListener, TcpStream};
use std::collections::HashMap;
use std::io::{self};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, Condvar};
use std::thread;
use std::time::Instant;
use log::{debug, info, trace};

use crate::client::Stream;
use crate::config::constants::MIN_CHUNK_SIZE;
use crate::mioserver::handlers::basic_handler::{handle_client_readable_data, handle_client_writable_data};
use crate::mioserver::handlers::greeting_handler::{handle_greeting_accep_token_read, handle_greeting_send_accept_token};
use crate::mioserver::ServerTestPhase;

const MAX_CONNECTIONS: usize = 1024;
const NUM_WORKER_THREADS: usize = 10;

pub struct WorkerThread {
    thread: thread::JoinHandle<()>,
}

pub struct MioServer {
    listener: TcpListener,
    worker_threads: Vec<WorkerThread>,
    task_queue: Arc<(Mutex<Vec<TcpStream>>, Condvar)>,
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
    fn new(id: usize, task_queue: Arc<(Mutex<Vec<TcpStream>>, Condvar)>) -> io::Result<Self> {
        println!("Worker {} new", id);

        let thread = thread::Builder::new()
            .stack_size(8 * 1024 * 1024) // 8MB stack
            .spawn(move || {
                println!("Worker {}: starting", id);
                let mut worker = Worker::new(id, task_queue).expect("Failed to create worker");
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
    task_queue: Arc<(Mutex<Vec<TcpStream>>, Condvar)>,
    next_token: usize,
}

impl Worker {
    fn new(id: usize, task_queue: Arc<(Mutex<Vec<TcpStream>>, Condvar)>) -> io::Result<Self> {
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
            task_queue,
            next_token: 1,
        })
    }
    
    fn run(&mut self) -> io::Result<()> {
        println!("Worker {}: entering run method", self.id);
        
        loop {
            // Ждем новую задачу
            let mut stream = self.wait_for_task()?;
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
            
            debug!("Worker {} registered new connection with token {:?}", self.id, token);
            
            // Обрабатываем соединение
            self.process_connection(token)?;
        }
    }
    
    fn wait_for_task(&self) -> io::Result<TcpStream> {
        let (queue, cvar) = &*self.task_queue;
        
        loop {
            let mut queue = queue.lock().unwrap();
            
            if let Some(stream) = queue.pop() {
                return Ok(stream);
            }
            
            // Ждем, пока появится задача
            queue = cvar.wait(queue).unwrap();
        }
    }
    
    fn process_connection(&mut self, token: Token) -> io::Result<()> {
        println!("Worker {}: starting to process connection {:?}", self.id, token);
        
        loop {
            // Обрабатываем события с таймаутом
            if let Err(e) = self.poll.poll(&mut self.events, Some(std::time::Duration::from_millis(1000))) {
                info!("Worker {}: Poll error: {}", self.id, e);
                return Err(e);
            }
            for event in self.events.iter() {
                let event_token = event.token();
                if event_token == token {
                    let mut should_remove = false;
                    
                    if let Some(state) = self.connections.get_mut(&token) {
                        if event.is_readable() {
                            trace!("Worker {}: event is readable", self.id);
                            should_remove = handle_client_readable_data(state, &self.poll).is_err();
                        }
                        if event.is_writable() {
                            trace!("Worker {}: event is writable", self.id);
                            should_remove = handle_client_writable_data(state, &self.poll).is_err();
                        }
                    }
                    
                    if should_remove {
                        info!("Worker {}: Error handling client data for token {:?}", self.id, token);
                        self.connections.remove(&token);
                        return Ok(());
                    }
                }
            }
            
            // Проверяем, не завершилось ли соединение
            if !self.connections.contains_key(&token) {
                println!("Worker {}: connection {:?} closed, stopping processing", self.id, token);
                return Ok(());
            }
        }
    }
}


impl MioServer {
    pub fn new(addr: SocketAddr) -> io::Result<Self> {
        let listener = TcpListener::bind(addr)?;
        
        // Создаем очередь задач
        let task_queue = Arc::new((Mutex::new(Vec::new()), Condvar::new()));
        
        // Создаем воркеры
        let mut worker_threads = Vec::new();
        
        for i in 0..NUM_WORKER_THREADS {
            let worker = WorkerThread::new(i, task_queue.clone())?;
            worker_threads.push(worker);
        }
        
        debug!("MIO TCP Server started on {} with {} worker threads", addr, NUM_WORKER_THREADS);
        
        Ok(Self {
            listener,
            worker_threads,
            task_queue,
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
                    
                    // Кладим соединение в очередь и будим один поток
                    let (queue, cvar) = &*self.task_queue;
                    {
                        let mut queue = queue.lock().unwrap();
                        queue.push(stream);
                    }
                    cvar.notify_one(); // Будим один поток
                    
                    debug!("Connection added to queue and worker notified");
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