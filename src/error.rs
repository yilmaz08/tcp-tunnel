use thiserror::Error;

#[derive(Debug, Error)]
pub enum TunnelError {
    #[error("Secret mismatch")]
    SecretMismatch,
    
    #[error("Timed out")]
    Timeout,

    #[error("Early EOF in nonce exchange (possible ban)")]
    NonceEarlyEOF,
}
