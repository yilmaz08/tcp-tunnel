use thiserror::Error;

#[derive(Debug, Error)]
pub enum TunnelError {
    #[error("Secret mismatch")]
    SecretMismatch,

    #[error("Timed out")]
    Timeout,

    #[error("Early EOF in nonce exchange (possible ban)")]
    NonceEarlyEOF,

    // Config Errors
    #[error("Endpoint wasn't not found")]
    EndpointNotFound,

    #[error("Endpoint is connected to itself")]
    RouteToSelf,
}
