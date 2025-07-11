#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerTestPhase {
    GreetingReceiveConnectionType,
    GreetingSendAcceptToken,
    GreetingReceiveToken,
    GreetingSendOk,
    GreetingSendChunksize,

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

    GetTimeSendChunk,
    GetTimeReceiveOk,
    GetTimeSendTime,

    PutNoResultSendOk,
    PutNoResultReceiveChunk,
    PutNoResultSendTime,

    PutSendOk,
    PutReceiveChunk,
    PutSendTime,
    PutSendBytes,
}