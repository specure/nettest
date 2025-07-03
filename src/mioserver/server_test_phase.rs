#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerTestPhase {
    GreetingReceiveConnectionType,
    GreetingSendAcceptToken,
    GreetingReceiveToken,
    GreetingSendOk,
}