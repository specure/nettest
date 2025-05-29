use anyhow::Result;
use mio::Token;
use crate::state::TestPhase;
use crate::handlers::{BasicHandler, GreetingHandler, GetChunksHandler, PingHandler, PutNoResultHandler};

pub struct HandlerFactory {
    greeting_handler: GreetingHandler,
    get_chunks_handler: GetChunksHandler,
    ping_handler: PingHandler,
    put_no_result_handler: PutNoResultHandler,
}

impl HandlerFactory {
    pub fn new(token: Token) -> Result<Self> {
        Ok(Self { 
            greeting_handler: GreetingHandler::new(token)?,
            get_chunks_handler: GetChunksHandler::new(token)?,
            ping_handler: PingHandler::new(token)?,
            put_no_result_handler: PutNoResultHandler::new(token)?,
        })
    }
    
    pub fn get_handler(&mut self, phase: &TestPhase) -> Option<&mut dyn BasicHandler> {
        match phase {
            TestPhase::GreetingReceiveGreeting => Some(&mut self.greeting_handler),
            TestPhase::GreetingSendConnectionType => Some(&mut self.greeting_handler),
            TestPhase::GreetingSendToken => Some(&mut self.greeting_handler),
            TestPhase::GreetingReceiveVersion => Some(&mut self.greeting_handler),
            TestPhase::GreetingReceiveAcceptToken => Some(&mut self.greeting_handler),
            TestPhase::GreetingReceiveOK => Some(&mut self.greeting_handler),
            
            TestPhase::GetChunksReceiveAccept => Some(&mut self.get_chunks_handler),
            TestPhase::GetChunksReceiveChunk => Some(&mut self.get_chunks_handler),
            TestPhase::GetChunksSendOk => Some(&mut self.get_chunks_handler),
            TestPhase::GetChunksReceiveTime => Some(&mut self.get_chunks_handler),
            TestPhase::GetChunksSendChunksCommand => Some(&mut self.get_chunks_handler),

            TestPhase::PingSendPing => Some(&mut self.ping_handler),
            TestPhase::PingReceivePong => Some(&mut self.ping_handler),
            TestPhase::PingSendOk => Some(&mut self.ping_handler),
            TestPhase::PingReceiveTime => Some(&mut self.ping_handler),

            TestPhase::GetTimeCommand => Some(&mut self.ping_handler),

            TestPhase::PutNoResultReceiveAccept => Some(&mut self.put_no_result_handler),
            TestPhase::PutNoResultSendCommand => Some(&mut self.put_no_result_handler),
            TestPhase::PutNoResultReceiveOk => Some(&mut self.put_no_result_handler),
            TestPhase::PutNoResultSendChunks => Some(&mut self.put_no_result_handler),
            TestPhase::PutNoResultReceiveTime => Some(&mut self.put_no_result_handler),
           
            TestPhase::End => None,
        }
    }
} 