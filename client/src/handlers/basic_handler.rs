use anyhow::Result;
use mio::{net::TcpStream, Interest, Poll, Token};
use crate::{state::{MeasurementState, TestPhase}};
use crate::stream::Stream;

pub trait BasicHandler {
    // Обработка события чтения
    fn on_read(&mut self, stream: &mut Stream, poll: &Poll, measurement_state: &mut MeasurementState) -> Result<()>;
    
    // Обработка события записи
    fn on_write(&mut self, stream: &mut Stream, poll: &Poll, measurement_state: &mut MeasurementState) -> Result<()> ;

} 