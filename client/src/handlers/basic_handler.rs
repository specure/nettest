use anyhow::Result;
use mio::{net::TcpStream, Interest, Poll, Token};
use crate::state::TestPhase;

pub trait BasicHandler {
    // Обработка события чтения
    fn on_read(&mut self, stream: &mut TcpStream, poll: &Poll) -> Result<()>;
    
    // Обработка события записи
    fn on_write(&mut self, stream: &mut TcpStream, poll: &Poll) -> Result<()> ;

    fn get_phase(&self) -> TestPhase;
} 