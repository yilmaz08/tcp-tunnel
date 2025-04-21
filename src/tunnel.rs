use super::error::TunnelError;
use anyhow::Result;
use chacha20::{
    cipher::{KeyIvInit, StreamCipher},
    ChaCha20,
};
use log::error;
use tokio::{
    io::{split, AsyncReadExt, AsyncWriteExt, ReadHalf, WriteHalf},
    net::TcpStream,
    task,
    time::{timeout, Duration},
};

// Starting bytes:
// 0x01 -> OK
// 0x02 -> SecretMismatch

const AUTH_TIMEOUT: Duration = Duration::from_secs(5);
const NONCE_TIMEOUT: Duration = Duration::from_secs(5);

pub struct Tunnel {
    nonce: [u8; 12],
    secret: [u8; 32],
    tunnel_read: ReadHalf<TcpStream>,
    tunnel_write: WriteHalf<TcpStream>,
    profile: bool,
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
                match timeout(AUTH_TIMEOUT, stream.read_exact(&mut auth)).await {
                    Ok(read) => {
                        read?;
                    }
                    Err(_) => return Err(TunnelError::Timeout.into()),
                }
                cipher.apply_keystream(&mut auth);
                // Verify
                if auth != *b"AUTH" {
                    stream.write_u8(2u8).await?; // send 0x02 to indicate SecretMismatch error
                    return Err(TunnelError::SecretMismatch.into());
                }

                nonce
            }
            false => {
                // Receive Nonce
                let mut nonce = [0u8; 12];
                match timeout(NONCE_TIMEOUT, stream.read_exact(&mut nonce)).await {
                    Ok(Ok(_)) => {}
                    Ok(Err(e)) => {
                        if e.kind() == std::io::ErrorKind::UnexpectedEof {
                            return Err(TunnelError::NonceEarlyEOF.into());
                        }
                        return Err(e.into());
                    }
                    Err(_) => return Err(TunnelError::Timeout.into()),
                }
                // Create cipher
                let mut cipher: ChaCha20 = ChaCha20::new(&secret.into(), &nonce.into());
                // Send encrypted "AUTH"
                let mut auth = *b"AUTH";
                cipher.apply_keystream(&mut auth);
                stream.write(&auth).await?;
                // Wait a starting byte
                if stream.read_u8().await? == 2u8 {
                    return Err(TunnelError::SecretRejected.into());
                }

                nonce
            }
        };

        let (tunnel_read, tunnel_write) = split(stream);
        Ok(Self {
            nonce,
            secret,
            tunnel_read,
            tunnel_write,
            profile,
        })
    }

    // Connect to separate tunnels to each other
    // 1- Create ciphers (4 in total)
    // 2- Start read_write function
    pub async fn join(mut self, mut b: Tunnel) -> Result<()> {
        let a_write = ChaCha20::new(&self.secret.into(), &self.nonce.into());
        let a_read = ChaCha20::new(&self.secret.into(), &self.nonce.into());

        let b_write = ChaCha20::new(&b.secret.into(), &b.nonce.into());
        let b_read = ChaCha20::new(&b.secret.into(), &b.nonce.into());

        if self.profile {
            self.tunnel_write.write_u8(1u8).await?;
        }
        if b.profile {
            b.tunnel_write.write_u8(1u8).await?;
        }

        let mut a_to_b = task::spawn(Tunnel::read_write(
            self.tunnel_read,
            b.tunnel_write,
            vec![a_read, b_write],
        ));

        let mut b_to_a = task::spawn(Tunnel::read_write(
            b.tunnel_read,
            self.tunnel_write,
            vec![b_read, a_write],
        ));

        tokio::select! {
            _ = &mut a_to_b => b_to_a.abort(),
            _ = &mut b_to_a => a_to_b.abort()
        }

        Ok(())
    }

    // Start data stream
    // 1- Create ciphers
    // 2- Start read_write function
    pub async fn run(mut self, stream: TcpStream) -> Result<()> {
        if self.profile {
            self.tunnel_write.write_u8(1u8).await?;
        }
        
        let (target_read, target_write) = split(stream);

        let tunnel_cipher = ChaCha20::new(&self.secret.into(), &self.nonce.into());
        let target_cipher = ChaCha20::new(&self.secret.into(), &self.nonce.into());

        let mut tunnel_to_target = task::spawn(Tunnel::read_write(
            self.tunnel_read,
            target_write,
            vec![tunnel_cipher],
        ));
        let mut target_to_tunnel = task::spawn(Tunnel::read_write(
            target_read,
            self.tunnel_write,
            vec![target_cipher],
        ));

        tokio::select! {
            _ = &mut tunnel_to_target => target_to_tunnel.abort(),
            _ = &mut target_to_tunnel => tunnel_to_target.abort()
        }

        Ok(())
    }

    // Read from a stream and write to another
    // 1- Read from the read half
    // 2- Apply key streams
    // 3- Write to the write half
    pub async fn read_write(
        mut read_stream: ReadHalf<TcpStream>,
        mut write_stream: WriteHalf<TcpStream>,
        mut ciphers: Vec<ChaCha20>,
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
                    for cipher in &mut ciphers {
                        cipher.apply_keystream(&mut buffer[..n]);
                    }
                    let _ = write_stream.write_all(&buffer[..n]).await;
                }
            }
        }
    }
}
