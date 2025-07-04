#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerTestPhase {
    GreetingReceiveConnectionType,
    GreetingSendAcceptToken,
    GreetingReceiveToken,
    GreetingSendOk,

    AcceptTokenQuit,
    AcceptCommandReceive,
    AcceptCommandSend,

    GetChunkSendOk,
    GetChunkSendChunk,
    GetChunksReceiveOK,
    GetChunksSendTime,

    PongSend,
    PingReceiveOk,
    PingSendTime,
}