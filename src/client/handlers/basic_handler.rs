use log::debug;
use mio::{Poll};
use std::io;

use crate::client::handlers::get_chunks::{handle_get_chunks_receive_chunk, handle_get_chunks_receive_time, handle_get_chunks_send_chunks_command, handle_get_chunks_send_ok};
use crate::client::handlers::greeting::{handle_greeting_receive_greeting, handle_greeting_receive_response, handle_greeting_send_connection_type, handle_greeting_send_token,};
use crate::client::handlers::get_time::{handle_get_time_receive_chunk, handle_get_time_receive_time, handle_get_time_send_command, handle_get_time_send_ok};
use crate::client::handlers::perf::{handle_perf_receive_ok, handle_perf_receive_time, handle_perf_send_chunks, handle_perf_send_command, handle_perf_send_last_chunk};
use crate::client::handlers::ping::{handle_ping_receive_pong, handle_ping_receive_time, handle_ping_send_ok, handle_ping_send_ping};
use crate::client::handlers::puttimeresult::{handle_put_time_result_send_command, handle_put_time_result_send_chunks, handle_put_time_result_send_last_chunk, handle_put_time_result_receive_ok, handle_put_time_result_receive_time};
use crate::client::state::{MeasurementState, TestPhase};


pub fn handle_client_readable_data(state: &mut MeasurementState, poll: &Poll) -> io::Result<usize> {

    match state.phase {
        TestPhase::GreetingReceiveGreeting => handle_greeting_receive_greeting(poll, state),
        TestPhase::GreetingReceiveResponse => handle_greeting_receive_response(poll, state),
       
        TestPhase::GetChunksReceiveChunk => handle_get_chunks_receive_chunk(poll, state),
        TestPhase::GetChunksReceiveTime => handle_get_chunks_receive_time(poll, state),

        TestPhase::GetTimeReceiveChunk => handle_get_time_receive_chunk(poll, state),
        TestPhase::GetTimeReceiveTime => handle_get_time_receive_time(poll, state),

        TestPhase::PingReceivePong => handle_ping_receive_pong(poll, state),
        TestPhase::PingReceiveTime => handle_ping_receive_time(poll, state),

        TestPhase::PerfReceiveOk => handle_put_time_result_receive_ok(poll, state),
        TestPhase::PerfReceiveTime => handle_put_time_result_receive_time(poll, state),

        // TestPhase::PerfReceiveOk => handle_perf_receive_ok(poll, state),
        // TestPhase::PerfReceiveTime => handle_perf_receive_time(poll, state),

        _ => {
            debug!("Unknown read phase: {:?}", state.phase);
            return Ok(1);
        },
    }
}

pub fn handle_client_writable_data(state: &mut MeasurementState, poll: &Poll) -> io::Result<usize> {

    match state.phase {
        TestPhase::GreetingSendConnectionType => handle_greeting_send_connection_type(poll, state),
        TestPhase::GreetingSendToken => handle_greeting_send_token(poll, state),
        
        TestPhase::GetChunksSendChunksCommand => handle_get_chunks_send_chunks_command(poll, state),
        TestPhase::GetChunksSendOk => handle_get_chunks_send_ok(poll, state),

        TestPhase::PingSendPing => handle_ping_send_ping(poll, state),
        TestPhase::PingSendOk => handle_ping_send_ok(poll, state),

        TestPhase::GetTimeSendCommand => handle_get_time_send_command(poll, state),
        TestPhase::GetTimeSendOk => handle_get_time_send_ok(poll, state),

        TestPhase::PerfSendCommand => handle_put_time_result_send_command(poll, state),
        TestPhase::PerfSendChunks => handle_put_time_result_send_chunks(poll, state),
        TestPhase::PerfSendLastChunk => handle_put_time_result_send_last_chunk(poll, state),

        // TestPhase::PerfSendCommand => handle_perf_send_command(poll, state),
        // TestPhase::PerfSendChunks => handle_perf_send_chunks(poll, state),
        // TestPhase::PerfSendLastChunk => handle_perf_send_last_chunk(poll, state),

        _ => {
            debug!("Unknown write phase: {:?}", state.phase);
            return Ok(1);
        },
    }
}