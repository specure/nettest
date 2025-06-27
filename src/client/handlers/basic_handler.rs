use anyhow::Result;
use mio::{Poll};
use crate::{client::MeasurementState, client::Stream};

pub trait BasicHandler {
    // Обработка события чтения
    fn on_read(&mut self, stream: &mut Stream, poll: &Poll, measurement_state: &mut MeasurementState) -> Result<()>;
    
    // Обработка события записи
    fn on_write(&mut self, stream: &mut Stream, poll: &Poll, measurement_state: &mut MeasurementState) -> Result<()> ;

} 