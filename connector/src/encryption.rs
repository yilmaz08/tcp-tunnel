use sha2::{Sha256, Digest};

pub fn generate_secret_from_string(secret_str: String) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(secret_str);
    let mut secret = [0u8; 32];
    secret.copy_from_slice(&hasher.finalize());
    secret
}
