#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerTestPhase {
    GreetingReceiveConnectionType,
    GreetingSendVersion,
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
    GetChunksSendChunksLast,
    GetChunksSendTime,

    PongSend,
    PingReceiveOk,
    PingSendTime,

    GetTimeSendChunk,

    GetTimeSendLastChunk,

    GetTimeReceiveOk,
    GetTimeSendTime,

    PutNoResultSendOk,
    PutNoResultReceiveChunk,
    PutNoResultSendTime,

    PutSendOk,
    PutReceiveChunk,
    PutSendBytes,
    PutSendTime,

    PutTimeResultSendOk,
    PutTimeResultReceiveChunk,
    PutTimeResultSendTimeResult,
}