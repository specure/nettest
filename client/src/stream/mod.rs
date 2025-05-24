use log::{debug, error, info};
use mio::{Events, Interest, Poll, Token};
use mio::net::TcpStream as MioTcpStream;
use std::io::{self, Read, Write, IoSlice};
use std::net::SocketAddr;
use std::time::Duration;

const CHUNK_SIZE: usize = 4096;
const BUFFER_SIZE: usize = 8192;
const CLIENT: Token = Token(0);



#[derive(Debug)]
pub enum Stream {
    Plain(TcpStream),
}



impl Stream { 
    pub fn new(addr: SocketAddr) -> io::Result<Self> {

        
        let stream = TcpStream::new(addr)?;
        Ok(Stream::Plain(stream))
    }



    pub async fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        // sleep(Duration::from_millis(50)).await;
        match self {
            Stream::Plain(stream) => stream.read(buf)
        }
    }

    pub async fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Stream::Plain(stream) => stream.write(buf)
        }
    }

    pub async fn write_all(&mut self, buf: &[u8]) -> std::io::Result<()> {
        match self {
            Stream::Plain(stream) => stream.write_all(buf),
        }
    }

    pub async fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Stream::Plain(stream) => stream.flush(),
        }
    }

    pub fn to_string(&self) -> String {
        match self {
            Stream::Plain(_) => "Plain".to_string(),
        }
    }
}




#[derive(Debug)]
pub struct TcpStream {
    stream: MioTcpStream,
    poll: Poll,
    events: Events,
}

impl TcpStream {
    pub fn new(addr: SocketAddr) -> io::Result<Self> {
        info!("Creating new TcpStream for address: {}", addr);
        let mut stream = MioTcpStream::connect(addr)?;
        info!("TcpStream connected successfully");
        
        let mut poll = Poll::new()?;    
        let events = Events::with_capacity(128);

        // TCP optimizations
        stream.set_nodelay(true)?;
        info!("TCP_NODELAY set to true");

        // Register socket for event monitoring
        info!("Registering socket for event monitoring...");
        poll.registry().register(
            &mut stream,
            CLIENT,
            Interest::READABLE | Interest::WRITABLE,
        )?;
        info!("Socket registered successfully");

        Ok(Self {
            stream,
            poll,
            events,
        })
    }

    pub fn wait_for_readable(&mut self) -> io::Result<()> {
        info!("Waiting for socket to become readable...");
        loop {
            self.poll_events()?;
            if self.is_readable() {
                info!("Socket is now readable");
                return Ok(());
            }
            info!("Socket not readable yet, waiting...");
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    pub fn read_nonblocking(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        info!("Attempting non-blocking read...");
        self.wait_for_readable()?;
        match self.stream.read(buf) {
            Ok(n) => {
                info!("Non-blocking read successful, read {} bytes", n);
                Ok(n)
            },
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                info!("Non-blocking read would block");
                Ok(0)
            },
            Err(e) => {
                error!("Non-blocking read error: {}", e);
                Err(e)
            }
        }
    }

    pub fn write_nonblocking(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self.stream.write(buf) {
            Ok(n) => Ok(n),
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => Ok(0),
            Err(e) => Err(e),
        }
    }

    pub fn is_readable(&self) -> bool {
        let readable = self.events.iter().any(|event| {
             event.is_readable()
        });
        info!("Checking if socket is readable: {}", readable);
        readable
    }

    pub fn is_writable(&self) -> bool {
        let writable = self.events.iter().any(|event| {
            event.token() == CLIENT && event.is_writable()
        });
        info!("Checking if socket is writable: {}", writable);
        writable
    }

    pub fn poll_events(&mut self) -> io::Result<()> {
        info!("Polling for events with timeout 1000ms...");
        match self.poll.poll(&mut self.events, Some(Duration::from_millis(1000))) {
            Ok(_) => {
                info!("Poll completed");
                for event in self.events.iter() {
                    info!("Event details: token={}, readable={}, writable={}, error={}", 
                        event.token().0,
                        event.is_readable(),
                        event.is_writable(),
                        event.is_error());
                }
                Ok(())
            },
            Err(e) => {
                error!("Error polling events: {}", e);
                Err(e)
            }
        }
    }

}

impl Read for TcpStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        info!("Polling events for read...");
        let start = std::time::Instant::now();
        let timeout = Duration::from_secs(5);

        loop {
            if start.elapsed() > timeout {
                error!("Timeout waiting for socket to become readable");
                return Err(io::Error::new(io::ErrorKind::TimedOut, "Read timeout"));
            }

            self.poll_events()?;
            info!("Checking events...");
            
            for event in self.events.iter() {
                info!("Processing event: token={}, readable={}, writable={}", 
                      event.token().0,
                      event.is_readable(),
                      event.is_writable());
                
                if event.is_readable() {
                    info!("Socket is readable, attempting to read data...");
                    match self.read_nonblocking(buf) {
                        Ok(n) => {
                            info!("Successfully read {} bytes", n);
                            return Ok(n);
                        },
                        Err(e) => {
                            error!("Error reading data: {}", e);
                            return Err(e);
                        }
                    }
                }
            }
            
            info!("No readable events found, waiting...");
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

impl Write for TcpStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.poll_events()?;
        if self.is_writable() {
            let result = self.write_nonblocking(buf);
            self.events.clear();
            result
        } else {
            Err(io::Error::new(io::ErrorKind::WouldBlock, "Not writable"))
        }
    }

    fn write_all(&mut self, mut buf: &[u8]) -> io::Result<()> {
        while !buf.is_empty() {
            self.poll_events()?;
            if self.is_writable() {
                match self.write_nonblocking(buf) {
                    Ok(0) => {
                        continue;
                    }
                    Ok(n) => {
                        buf = &buf[n..];
                    }
                    Err(e) => return Err(e),
                }
            }
        }
        self.stream.flush()?;
        self.events.clear();
        Ok(())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stream.flush()
    }
}
