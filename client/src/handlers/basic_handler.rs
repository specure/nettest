use anyhow::Result;
use mio::{net::TcpStream, Interest, Poll, Token};
use crate::state::{MeasurementState, TestPhase};

pub trait BasicHandler {
    // Обработка события чтения
    fn on_read(&mut self, stream: &mut TcpStream, poll: &Poll, measurement_state: &mut MeasurementState) -> Result<()>;
    
    // Обработка события записи
    fn on_write(&mut self, stream: &mut TcpStream, poll: &Poll, measurement_state: &mut MeasurementState) -> Result<()> ;

} 