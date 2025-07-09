use crate::mioserver::{handlers::{common::{handle_main_command_receive, handle_main_command_send}, getchunks::{handle_get_chunks_receive_ok, handle_get_chunks_send_chunks, handle_get_chunks_send_ok, handle_get_chunks_send_time}, gettime::{handle_get_time_receive_ok, handle_get_time_send_chunk, handle_get_time_send_time}, greeting_handler::{handle_greeting_accep_token_read, handle_greeting_receive_token, handle_greeting_send_accept_token, handle_greeting_send_chunksize, handle_greeting_send_ok}, ping::{handle_ping_receive_ok, handle_ping_send_time, handle_pong_send}, put::{handle_put_receive_chunk, handle_put_send_bytes, handle_put_send_ok, handle_put_send_time}, putnoresult::{handle_put_no_result_receive_chunk, handle_put_no_result_send_ok, handle_put_no_result_send_time}}, server::TestState, ServerTestPhase};
use mio::Poll;
use std::io;
use log::{debug};


pub fn handle_client_readable_data(state: &mut TestState, poll: &Poll) -> io::Result<usize> {
    match state.measurement_state {
        ServerTestPhase::GreetingReceiveConnectionType => handle_greeting_accep_token_read(poll, state),
        ServerTestPhase::GreetingReceiveToken => handle_greeting_receive_token(poll, state),
       
        ServerTestPhase::GetChunksReceiveOK => handle_get_chunks_receive_ok(poll, state),
        
        ServerTestPhase::AcceptCommandReceive => handle_main_command_receive(poll, state),
       
        ServerTestPhase::GetTimeReceiveOk => handle_get_time_receive_ok(poll, state),

        ServerTestPhase::PingReceiveOk => handle_ping_receive_ok(poll, state),

        ServerTestPhase::PutNoResultReceiveChunk => handle_put_no_result_receive_chunk(poll, state),

        ServerTestPhase::PutReceiveChunk => handle_put_receive_chunk(poll, state),
        
        _ => {
            debug!("Unknown measurement state: {:?}", state.measurement_state);
            Ok(1)
        }
    }
}

pub fn handle_client_writable_data(state: &mut TestState, poll: &Poll) -> io::Result<usize> {
    match state.measurement_state {
       
        ServerTestPhase::GreetingSendAcceptToken => handle_greeting_send_accept_token(poll, state),
        ServerTestPhase::GreetingSendOk => handle_greeting_send_ok(poll, state),
        ServerTestPhase::GreetingSendChunksize => handle_greeting_send_chunksize(poll, state),
       
        ServerTestPhase::GetChunkSendOk => handle_get_chunks_send_ok(poll, state),
        ServerTestPhase::GetChunkSendChunk => handle_get_chunks_send_chunks(poll, state),
        ServerTestPhase::GetChunksSendTime => handle_get_chunks_send_time(poll, state),

        ServerTestPhase::AcceptCommandSend => handle_main_command_send(poll, state),

        ServerTestPhase::PongSend => handle_pong_send(poll, state),
        ServerTestPhase::PingSendTime => handle_ping_send_time(poll, state),

        ServerTestPhase::GetTimeSendChunk => handle_get_time_send_chunk(poll, state),
        ServerTestPhase::GetTimeSendTime => handle_get_time_send_time(poll, state),

        ServerTestPhase::PutNoResultSendOk => handle_put_no_result_send_ok(poll, state),
        ServerTestPhase::PutNoResultSendTime => handle_put_no_result_send_time(poll, state),

        ServerTestPhase::PutSendOk => handle_put_send_ok(poll, state),
        ServerTestPhase::PutSendTime => handle_put_send_time(poll, state),
        ServerTestPhase::PutSendBytes => handle_put_send_bytes(poll, state),

        _ => {
            debug!("Unknown measurement state: {:?}", state.measurement_state);
            Ok(1)
        }
    }
}

