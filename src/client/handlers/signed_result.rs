use log::debug;
use mio::{Interest, Poll};

use crate::client::state::{MeasurementState, TestPhase};

pub fn handle_signed_result_command(
    poll: &Poll,
    state: &mut MeasurementState,
) -> Result<usize, std::io::Error> {
    debug!("handle_signed_result_command");
    let command = b"SIGNEDRESULT";
    if state.write_pos == 0 {
        state.write_buffer[0..command.len()].copy_from_slice(command);

    }
    loop {
        let n = state.stream.write(&mut state.read_buffer)?;
        state.write_pos += n;
        if state.write_pos >= state.write_buffer.len() {
            state.write_pos = 0;
            state.read_pos = 0;
            state.stream.reregister(poll, state.token, Interest::READABLE)?;
            state.phase = TestPhase::SignedResultReceive;
            return Ok(n);
        }
    }
}

pub fn handle_signed_result_receive(
    poll: &Poll,
    state: &mut MeasurementState,
) -> Result<usize, std::io::Error> {
    debug!("handle_signed_result_receive");
    loop {
        let n = state.stream.read(&mut state.read_buffer[state.read_pos..])?;
        state.read_pos += n;
        let line = String::from_utf8_lossy(&state.read_buffer[0..state.read_pos]);
        if line.ends_with("\n") {
            state.envelope = Some(line.to_string());
            state.read_pos = 0;
            state.stream.reregister(poll, state.token, Interest::WRITABLE)?;
            state.phase = TestPhase::SignedResultSendOk;
            return Ok(n);
        }
    }
}

pub fn handle_signed_result_send_ok(
    poll: &Poll,
    state: &mut MeasurementState,
) -> Result<usize, std::io::Error> {
    debug!("handle_signed_result_send_ok");
    let ok = b"OK\n";
    loop {
        let n = state.stream.write(ok)?;
        state.write_pos += n;
        if state.write_pos >= ok.len() {
            state.write_pos = 0;
            state.read_pos = 0;
            state.stream.reregister(poll, state.token, Interest::READABLE)?;
            state.phase = TestPhase::SignedResultCompleted;
            return Ok(n);
        }
    }
}
