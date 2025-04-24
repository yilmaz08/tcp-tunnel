use thiserror::Error;

#[derive(Debug, Error)]
pub enum TunnelError {
    // Occurs on inbound tunnels and doesn't timeout
    #[error("Secret mismatch")]
    SecretMismatch(std::net::IpAddr),
    // Occurs on outbound tunnels and times out
    #[error("Secret rejected")]
    SecretRejected,

    #[error("Timed out")]
    Timeout(std::net::IpAddr),

    #[error("Early EOF in nonce exchange (possible ban)")]
    NonceEarlyEOF,

    #[error("Connection attempt from banned IP")]
    ConnAttemptFromBannedIP,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Endpoint wasn't not found")]
    EndpointNotFound,

    #[error("Endpoint is connected to itself")]
    RouteToSelf,

    #[error("Every tunnel requires a secret")]
    NoSecret,
}
