use crate::mioserver::{handlers::greeting_handler::{handle_greeting_accep_token_read, handle_greeting_receive_token, handle_greeting_send_accept_token, handle_greeting_send_ok}, server::TestState, ServerTestPhase};
use mio::Poll;
use std::io;
use log::debug;


pub fn handle_client_readable_data(state: &mut TestState, poll: &Poll) -> io::Result<()> {
    match state.measurement_state {
        ServerTestPhase::GreetingReceiveConnectionType => handle_greeting_accep_token_read(poll, state),
        ServerTestPhase::GreetingReceiveToken => handle_greeting_receive_token(poll, state),
        _ => {
            debug!("Unknown measurement state: {:?}", state.measurement_state);
            Ok(())
        }
    }
}

pub fn handle_client_writable_data(state: &mut TestState, poll: &Poll) -> io::Result<()> {
    match state.measurement_state {
        ServerTestPhase::GreetingSendAcceptToken => handle_greeting_send_accept_token(poll, state),
        ServerTestPhase::GreetingSendOk => handle_greeting_send_ok(poll, state),
        _ => {
            debug!("Unknown measurement state: {:?}", state.measurement_state);
            Ok(())
        }
    }
}