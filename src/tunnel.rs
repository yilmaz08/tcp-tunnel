use crate::error::TunnelError;
use anyhow::Result;
use chacha20::{
    cipher::{KeyIvInit, StreamCipher},
    ChaCha20,
};
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
    is_inbound: bool,
}

impl Tunnel {
    // Initializes the tunnel
    pub async fn init(mut stream: TcpStream, is_inbound: bool, secret: [u8; 32]) -> Result<Self> {
        let nonce = match is_inbound {
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
                    Err(_) => return Err(TunnelError::Timeout(stream.peer_addr()?.ip()).into()),
                }
                cipher.apply_keystream(&mut auth);
                // Verify
                if auth != *b"AUTH" {
                    stream.write_u8(2u8).await?; // send 0x02 to indicate SecretMismatch error
                    return Err(TunnelError::SecretMismatch(stream.peer_addr()?.ip()).into());
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
                    Err(_) => return Err(TunnelError::Timeout(stream.peer_addr()?.ip()).into()),
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
            is_inbound,
        })
    }

    // Connect the tunnel to another tunnel
    pub async fn join(mut self, mut other: Tunnel) -> Result<()> {
        // Send starting byte for inbound tunnels
        if self.is_inbound {
            self.tunnel_write.write_u8(1u8).await?;
        }
        if other.is_inbound {
            other.tunnel_write.write_u8(1u8).await?;
        }

        // Generate ciphers
        let self_read_cipher = ChaCha20::new(&self.secret.into(), &self.nonce.into());
        let self_write_cipher = ChaCha20::new(&self.secret.into(), &self.nonce.into());
        let other_read_cipher = ChaCha20::new(&other.secret.into(), &other.nonce.into());
        let other_write_cipher = ChaCha20::new(&other.secret.into(), &other.nonce.into());

        // Spawn tasks
        let mut self_to_other = task::spawn(Tunnel::read_write(
            self.tunnel_read,
            other.tunnel_write,
            vec![self_read_cipher, other_write_cipher],
        ));
        let mut other_to_self = task::spawn(Tunnel::read_write(
            other.tunnel_read,
            self.tunnel_write,
            vec![other_read_cipher, self_write_cipher],
        ));

        // Manage tasks
        tokio::select! {
            _ = &mut self_to_other => other_to_self.abort(),
            _ = &mut other_to_self => self_to_other.abort()
        }

        Ok(())
    }

    // Connect the tunnel to a TcpStream
    pub async fn run(mut self, stream: TcpStream) -> Result<()> {
        // Send starting byte for inbound tunnels
        if self.is_inbound {
            self.tunnel_write.write_u8(1u8).await?;
        }

        let (target_read, target_write) = split(stream);

        // Generate ciphers
        let read_cipher = ChaCha20::new(&self.secret.into(), &self.nonce.into());
        let write_cipher = ChaCha20::new(&self.secret.into(), &self.nonce.into());

        // Spawn tasks
        let mut tunnel_to_target = task::spawn(Tunnel::read_write(
            self.tunnel_read,
            target_write,
            vec![read_cipher],
        ));
        let mut target_to_tunnel = task::spawn(Tunnel::read_write(
            target_read,
            self.tunnel_write,
            vec![write_cipher],
        ));

        // Manage tasks
        tokio::select! {
            _ = &mut tunnel_to_target => target_to_tunnel.abort(),
            _ = &mut target_to_tunnel => tunnel_to_target.abort()
        }

        Ok(())
    }

    // Connect a TcpStream to another TcpStream
    pub async fn proxy(a: TcpStream, b: TcpStream) -> Result<()> {
        let (a_read, a_write) = split(a);
        let (b_read, b_write) = split(b);

        let mut a_to_b = tokio::task::spawn(Tunnel::read_write(a_read, b_write, vec![]));
        let mut b_to_a = tokio::task::spawn(Tunnel::read_write(b_read, a_write, vec![]));

        tokio::select! {
            _ = &mut a_to_b => b_to_a.abort(),
            _ = &mut b_to_a => a_to_b.abort()
        }

        Ok(())
    }

    // Read from a stream and write to another
    pub async fn read_write(
        mut read_stream: ReadHalf<TcpStream>,
        mut write_stream: WriteHalf<TcpStream>,
        mut ciphers: Vec<ChaCha20>,
    ) -> Result<()> {
        let mut buffer = vec![0u8; 8192];
        loop {
            // Read
            let n = read_stream.read(&mut buffer).await?;
            if n == 0 {
                // EOF
                write_stream.shutdown().await?;
                return Ok(());
            }

            // Apply keystreams
            for cipher in &mut ciphers {
                cipher.apply_keystream(&mut buffer[..n]);
            }

            // Write
            write_stream.write_all(&mut buffer[..n]).await?;
        }
    }
}
