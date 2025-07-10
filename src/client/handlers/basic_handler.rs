use anyhow::Result;
use mio::{Poll};
use std::io;

use crate::{client::MeasurementState};
use crate::stream::stream::Stream;

pub trait BasicHandler {
    // Обработка события чтения
    fn on_read(&mut self, poll: &Poll, measurement_state: &mut MeasurementState) -> Result<()>;
    
    // Обработка события записи
    fn on_write(&mut self,  poll: &Poll, measurement_state: &mut MeasurementState) -> Result<()> ;

} 


pub fn handle_client_readable_data(state: &mut MeasurementState, poll: &Poll) -> io::Result<usize> {
    return Ok(0);
}

pub fn handle_client_writable_data(state: &mut MeasurementState, poll: &Poll) -> io::Result<usize> {
    return Ok(0);
}