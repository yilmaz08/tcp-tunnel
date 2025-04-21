use thiserror::Error;

#[derive(Debug, Error)]
pub enum TunnelError {
    // Occurs on inbound tunnels and doesn't timeout
    #[error("Secret Mismatch")]
    SecretMismatch,
    // Occurs on outbound tunnels and times out
    #[error("Secret Mismatch")]
    SecretRejected,

    #[error("Timed out")]
    Timeout,

    #[error("Early EOF in nonce exchange (possible ban)")]
    NonceEarlyEOF,

    // Config Errors
    #[error("Endpoint wasn't not found")]
    EndpointNotFound,

    #[error("Endpoint is connected to itself")]
    RouteToSelf,

    #[error("Every tunnel requires a secret")]
    NoSecret,
}
