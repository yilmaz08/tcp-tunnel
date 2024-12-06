use tokio::net::TcpStream;
use tokio::task;
use tokio::io::{AsyncReadExt, AsyncWriteExt, split, ReadHalf, WriteHalf, AsyncBufReadExt, BufReader};
use chacha20::ChaCha20;
use chacha20::cipher::{KeyIvInit, StreamCipher};
use base64::{Engine, engine::general_purpose};
use anyhow::Result;
use crate::environment::Environment;
use log::{info, trace, error, debug};

pub struct Connection {
    nonce: [u8; 12],
    env: Environment,
    log_target: String
}

impl Connection {
    pub fn new(index: u16, env: Environment) -> Self {
        let result = Self {
            nonce: [0u8; 12],
            env,
            log_target: format!("conn #{}", index)
        };
        debug!(target: &result.log_target, "Connection constructed!");
        return result;
    }

    pub async fn start(&mut self) -> Result<()> {
        debug!(target: &self.log_target, "Connecting to relay...");
        let relay_stream = Connection::create_stream(self.env.relay_host.clone(), self.env.relay_port.clone()).await;
        info!(target: &self.log_target, "Connected to relay!");
        let relay_stream = match self.relay_connect(relay_stream).await {
            Ok(val) => val,
            Err(e) => { debug!(target: &self.log_target, "Drop: {:?}", e); return Ok(()); }
        };
        debug!(target: &self.log_target, "Waiting...");
        let relay_stream = match Connection::wait_starting_byte(relay_stream).await {
            Ok(stream) => { trace!(target: &self.log_target, "Received starting byte!"); stream },
            Err(e) => return Err(e)
        };
        debug!(target: &self.log_target, "Connecting to server...");
        let server_stream = Connection::create_stream(self.env.server_host.clone(), self.env.server_port.clone()).await;
        info!(target: &self.log_target, "Connected to server!");
        return self.start_data_stream(relay_stream, server_stream).await;
    }

    async fn create_stream(host: String, port: u16) -> TcpStream {
        return TcpStream::connect(format!("{}:{}", host, port)).await.unwrap();
    }

    async fn start_data_stream(&mut self, relay_stream: TcpStream, server_stream: TcpStream) -> Result<()> {
        let (relay_read, relay_write) = split(relay_stream);
        let (server_read, server_write) = split(server_stream);
        trace!(target: &self.log_target, "Streams splitted");

        let relay_cipher = ChaCha20::new(&self.env.secret.into(), &self.nonce.into());
        let server_cipher = ChaCha20::new(&self.env.secret.into(), &self.nonce.into());
        trace!(target: &self.log_target, "Ciphers created");

        let mut relay_to_server = task::spawn(Connection::read_write(relay_read, server_write, relay_cipher));
        trace!(target: &self.log_target, "Relay to server task spawned!");
        let mut server_to_relay = task::spawn(Connection::read_write(server_read, relay_write, server_cipher));
        trace!(target: &self.log_target, "Server to relay task spawned!");

        tokio::select! {
            _ = &mut server_to_relay => {
                debug!(target: &self.log_target, "Server to relay ended");
                relay_to_server.abort();
            },
            _ = &mut relay_to_server => {
                debug!(target: &self.log_target, "Relay to server ended");
                server_to_relay.abort();
            }
        }

        info!(target: &self.log_target, "Connection completed!");
        Ok(())
    }

    async fn wait_starting_byte(mut stream: TcpStream) -> Result<TcpStream> {
        let mut buffer = [0u8; 1];
        loop {
            match stream.read(&mut buffer).await {
                Ok(0) => return Err(anyhow::Error::msg("Connection closed!")),
                Err(_) => return Err(anyhow::Error::msg("Failed to read from stream!")),
                Ok(_) => {
                    if buffer[0] != 0u8 { return Ok(stream); }
                }
            }
        }
    }

    async fn read_write(mut read_stream: ReadHalf<TcpStream>, mut write_stream: WriteHalf<TcpStream>, mut cipher: ChaCha20) {
        let mut buffer = [0u8; 512];
        loop {
            match read_stream.read(&mut buffer).await {
                Ok(0) => break,
                Err(e) => { error!("Failed to read from stream: {}", e); break; }
                Ok(n) => {
                    cipher.apply_keystream(&mut buffer);
                    let _ = write_stream.write_all(&buffer[..n]).await;
                }
            }
        }
    }

    async fn relay_connect(&mut self, mut stream: TcpStream) -> Result<TcpStream> {
        // Get the Generated Nonce
        let mut reader = BufReader::new(&mut stream);

        // Read the first line
        let mut base64_nonce = String::new();
        if reader.read_line(&mut base64_nonce).await? > 0 {
            base64_nonce = base64_nonce.trim_end().to_string();
            trace!(target: &self.log_target, "Received a nonce!");
        } else {
            return Err(anyhow::Error::msg("Connection closed!"));
        }
        // Decode base64 encoded nonce
        let engine = general_purpose::STANDARD;
        self.nonce = engine.decode(base64_nonce).unwrap().try_into().unwrap();
        trace!(target: &self.log_target, "Decoded the nonce");

        // Create new cipher
        let mut cipher = ChaCha20::new(&self.env.secret.into(), &self.nonce.into());
        trace!(target: &self.log_target, "Cipher created");

        // Send `encoded(encrypted("AUTH"))\r\n` for verification
        let mut message: Vec<u8> = b"AUTH".to_vec();
        cipher.apply_keystream(&mut message);
        trace!(target: &self.log_target, "Encrypted the message");
        let encoded_message = engine.encode(&message);
        trace!(target: &self.log_target, "Encoded the message");
        stream.write(encoded_message.as_bytes()).await?;
        stream.write(b"\r\n").await?;
        trace!(target: &self.log_target, "Sent the message");

        return Ok(stream);
    }
}
