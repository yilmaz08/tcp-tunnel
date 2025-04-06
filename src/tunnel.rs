use anyhow::Result;
use chacha20::{
    cipher::{KeyIvInit, StreamCipher},
    ChaCha20,
};
use log::error;
use tokio::{
    io::{split, AsyncReadExt, AsyncWriteExt, ReadHalf, WriteHalf},
    net::TcpStream,
};

pub struct Tunnel {
    nonce: [u8; 12],
    secret: [u8; 32],
    profile: bool, // true -> relay, false -> connector
    tunnel_read: ReadHalf<TcpStream>,
    tunnel_write: WriteHalf<TcpStream>,
}

impl Tunnel {
    // Initializes the tunnel
    // 1- Nonce exchange
    // 2- Authentication
    pub async fn init(mut stream: TcpStream, profile: bool, secret: [u8; 32]) -> Result<Self> {
        let nonce = match profile {
            true => {
                // Send Nonce
                let nonce = super::encryption::generate_random_nonce();
                stream.write(&nonce).await?;
                // Create cipher
                let mut cipher: ChaCha20 = ChaCha20::new(&secret.into(), &nonce.into());
                // Receive encrypted "AUTH"
                let mut auth = [0u8; 4];
                stream.read_exact(&mut auth).await?;
                cipher.apply_keystream(&mut auth);
                // Verify
                if auth != *b"AUTH" {
                    return Err(anyhow::Error::msg("Secret mismatch"));
                }

                nonce
            }
            false => {
                // Receive Nonce
                let mut nonce = [0u8; 12];
                stream.read_exact(&mut nonce).await?;
                // Create cipher
                let mut cipher: ChaCha20 = ChaCha20::new(&secret.into(), &nonce.into());
                // Send encrypted "AUTH"
                let mut auth = *b"AUTH";
                cipher.apply_keystream(&mut auth);
                stream.write(&auth).await?;
                // Wait a starting byte
                stream.read_u8().await?;

                nonce
            }
        };

        let (tunnel_read, tunnel_write) = split(stream);
        Ok(Self {
            nonce,
            secret,
            profile,
            tunnel_read,
            tunnel_write,
        })
    }

    // Start data stream
    // 1- Create ciphers
    // 2- Start read_write mirroring
    pub async fn run(mut self, stream: TcpStream) -> Result<()> {
        let (target_read, target_write) = split(stream);

        if self.profile {
            // Send starting byte
            self.tunnel_write.write_u8(1u8).await?;
        }

        let tunnel_cipher = ChaCha20::new(&self.secret.into(), &self.nonce.into());
        let target_cipher = ChaCha20::new(&self.secret.into(), &self.nonce.into());

        let mut tunnel_to_target = tokio::task::spawn(Tunnel::read_write(
            self.tunnel_read,
            target_write,
            tunnel_cipher,
        ));
        let mut target_to_tunnel = tokio::task::spawn(Tunnel::read_write(
            target_read,
            self.tunnel_write,
            target_cipher,
        ));

        tokio::select! {
            _ = &mut tunnel_to_target => target_to_tunnel.abort(),
            _ = &mut target_to_tunnel => tunnel_to_target.abort()
        }

        Ok(())
    }

    // Read from a stream and write to another
    // 1- Read from the read half
    // 2- Apply key stream
    // 3- Write to the write half
    async fn read_write(
        mut read_stream: ReadHalf<TcpStream>,
        mut write_stream: WriteHalf<TcpStream>,
        mut cipher: ChaCha20,
    ) {
        let mut buffer = [0u8; 512];
        loop {
            match read_stream.read(&mut buffer).await {
                Ok(0) => break,
                Err(e) => {
                    error!("Failed to read from stream: {}", e);
                    break;
                }
                Ok(n) => {
                    cipher.apply_keystream(&mut buffer);
                    let _ = write_stream.write_all(&buffer[..n]).await;
                }
            }
        }
    }
}
