use crate::handlers::{
    BasicHandler, GetChunksHandler, GetTimeHandler, GreetingHandler, PingHandler, PutHandler,
    PutNoResultHandler,
};
use crate::state::TestPhase;
use anyhow::Result;
use mio::Token;

pub struct HandlerFactory {
    greeting_handler: GreetingHandler,
    get_chunks_handler: GetChunksHandler,
    ping_handler: PingHandler,
    put_no_result_handler: PutNoResultHandler,
    put_handler: PutHandler,
    get_time_handler: GetTimeHandler,
}

impl HandlerFactory {
    pub fn new(token: Token) -> Result<Self> {
        Ok(Self {
            greeting_handler: GreetingHandler::new(token)?,
            get_chunks_handler: GetChunksHandler::new(token)?,
            ping_handler: PingHandler::new(token)?,
            put_no_result_handler: PutNoResultHandler::new(token)?,
            put_handler: PutHandler::new(token)?,
            get_time_handler: GetTimeHandler::new(token)?,
        })
    }

    pub fn get_handler(&mut self, phase: &TestPhase) -> Option<&mut dyn BasicHandler> {
        match phase {
            TestPhase::GreetingReceiveGreeting => Some(&mut self.greeting_handler),
            TestPhase::GreetingSendConnectionType => Some(&mut self.greeting_handler),
            TestPhase::GreetingSendToken => Some(&mut self.greeting_handler),
            TestPhase::GreetingReceiveResponse => Some(&mut self.greeting_handler),
            TestPhase::GreetingCompleted => None,

            TestPhase::GetTimeSendCommand => Some(&mut self.get_time_handler),
            TestPhase::GetTimeReceiveChunk => Some(&mut self.get_time_handler),
            TestPhase::GetTimeSendOk => Some(&mut self.get_time_handler),
            TestPhase::GetTimeReceiveTime => Some(&mut self.get_time_handler),
            TestPhase::GetTimeCompleted => None,


            TestPhase::GetChunksReceiveChunk => Some(&mut self.get_chunks_handler),
            TestPhase::GetChunksSendOk => Some(&mut self.get_chunks_handler),
            TestPhase::GetChunksReceiveTime => Some(&mut self.get_chunks_handler),
            TestPhase::GetChunksSendChunksCommand => Some(&mut self.get_chunks_handler),
            TestPhase::GetChunksCompleted => None,

            TestPhase::PingSendPing => Some(&mut self.ping_handler),
            TestPhase::PingReceivePong => Some(&mut self.ping_handler),
            TestPhase::PingSendOk => Some(&mut self.ping_handler),
            TestPhase::PingReceiveTime => Some(&mut self.ping_handler),
            TestPhase::PingCompleted => None,

            TestPhase::PutNoResultSendCommand => Some(&mut self.put_no_result_handler),
            TestPhase::PutNoResultReceiveOk => Some(&mut self.put_no_result_handler),
            TestPhase::PutNoResultSendChunks => Some(&mut self.put_no_result_handler),
            TestPhase::PutNoResultReceiveTime => Some(&mut self.put_no_result_handler),
            TestPhase::PutNoResultCompleted => None,
            TestPhase::PutSendCommand => Some(&mut self.put_handler),
            TestPhase::PutReceiveOk => Some(&mut self.put_handler),
            TestPhase::PutSendChunks => Some(&mut self.put_handler),
            TestPhase::PutReceiveTime => Some(&mut self.put_handler),
            TestPhase::PutReceiveBytesTime => Some(&mut self.put_handler),
            TestPhase::PutQuit => Some(&mut self.put_handler),
            TestPhase::PutCompleted => None,
        }
    }
}
