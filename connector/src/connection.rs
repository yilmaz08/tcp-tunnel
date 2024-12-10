use tokio::net::{TcpStream, UdpSocket};
use tokio::task;
use tokio::io::{AsyncReadExt, AsyncWriteExt, split, ReadHalf, WriteHalf, AsyncBufReadExt, BufReader};
use chacha20::ChaCha20;
use chacha20::cipher::{KeyIvInit, StreamCipher};
use base64::{Engine, engine::general_purpose};
use anyhow::Result;
use crate::environment::Environment;
use log::{info, trace, error, debug};
use std::sync::Arc;

pub enum TcpOrUdp {
    Udp(UdpSocket),
    Tcp(TcpStream)
}

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
        let mut relay_stream: TcpStream = match self.connect_to_relay().await {
            Ok(val) => val,
            Err(e) => { debug!(target: &self.log_target, "Drop: {:?}", e); return Ok(()); }
        };
        let server_stream: TcpOrUdp = match self.connect_to_server(&mut relay_stream).await {
            Ok(val) => val,
            Err(e) => return Err(e)
        };
        return self.start_data_stream(relay_stream, server_stream).await;
    }

    async fn connect_to_server(&mut self, mut relay_stream: &mut TcpStream) -> Result<TcpOrUdp> {
        // Wait a starting byte
        debug!(target: &self.log_target, "Waiting...");
        match Connection::wait_starting_byte(&mut relay_stream).await {
            Ok(_) => trace!(target: &self.log_target, "Received starting byte!"),
            Err(e) => return Err(e)
        };
        // Connect (UDP or TCP)
        debug!(target: &self.log_target, "Connecting to server...");
        let result: TcpOrUdp = match self.env.conn_protocol {
            true => TcpOrUdp::Tcp(TcpStream::connect(format!("{}:{}", self.env.server_host, self.env.server_port)).await.unwrap()),
            false => {
                let socket: UdpSocket = UdpSocket::bind(format!("{}:{}", self.env.server_host, 8008)).await.unwrap();
                socket.connect(format!("{}:{}", self.env.server_host, self.env.server_port)).await.unwrap();
                TcpOrUdp::Udp(socket)
            }
        };
        info!(target: &self.log_target, "Connected to server!");
        return Ok(result);
    }

    async fn connect_to_relay(&mut self) -> Result<TcpStream> {
        // Connect to Server
        debug!(target: &self.log_target, "Connecting to relay...");
        let mut stream = TcpStream::connect(format!("{}:{}", self.env.relay_host.clone(), self.env.relay_port.clone())).await.unwrap();
        info!(target: &self.log_target, "Connected to relay!");

        // Get the Nonce

        // // Read the first line
        let mut reader = BufReader::new(&mut stream);
        let mut base64_nonce = String::new();
        if reader.read_line(&mut base64_nonce).await? > 0 {
            base64_nonce = base64_nonce.trim_end().to_string();
            trace!(target: &self.log_target, "Received a nonce!");
        } else {
            return Err(anyhow::Error::msg("Connection closed!"));
        }
        // // Decode base64 encoded nonce
        let engine = general_purpose::STANDARD;
        self.nonce = engine.decode(base64_nonce).unwrap().try_into().unwrap();
        trace!(target: &self.log_target, "Decoded the nonce");

        // Authenticate
        
        // // Create new cipher
        let mut cipher = ChaCha20::new(&self.env.secret.into(), &self.nonce.into());
        trace!(target: &self.log_target, "Cipher created");

        // // Send `encoded(encrypted("AUTH"))\r\n` for verification
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

    async fn start_data_stream(&mut self, relay_stream: TcpStream, server_stream: TcpOrUdp) -> Result<()> {
        let relay_cipher = ChaCha20::new(&self.env.secret.into(), &self.nonce.into());
        let server_cipher = ChaCha20::new(&self.env.secret.into(), &self.nonce.into());
        trace!(target: &self.log_target, "Ciphers created");

        let (relay_read, relay_write) = split(relay_stream);
        let (mut relay_to_server, mut server_to_relay) = match server_stream {
            TcpOrUdp::Udp(socket) => {
                let shared_socket = Arc::new(socket);
                (task::spawn(Connection::read_write_t2u(relay_read, shared_socket.clone(), relay_cipher)),
                task::spawn(Connection::read_write_u2t(shared_socket, relay_write, server_cipher)))
            },
            TcpOrUdp::Tcp(stream) => {
                let (server_read, server_write) = split(stream);
                
                (task::spawn(Connection::read_write_t2t(relay_read, server_write, relay_cipher)),
                task::spawn(Connection::read_write_t2t(server_read, relay_write, server_cipher)))
            }
        };

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

    async fn wait_starting_byte(stream: &mut TcpStream) -> Result<()> {
        let mut buffer = [0u8; 1];
        loop {
            match stream.read(&mut buffer).await {
                Ok(0) => return Err(anyhow::Error::msg("Connection closed!")),
                Err(_) => return Err(anyhow::Error::msg("Failed to read from stream!")),
                Ok(_) => {
                    if buffer[0] != 0u8 { return Ok(()); }
                }
            }
        }
    }

    async fn read_write_t2t(mut read_stream: ReadHalf<TcpStream>, mut write_stream: WriteHalf<TcpStream>, mut cipher: ChaCha20) {
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

    async fn read_write_u2t(read_stream: Arc<UdpSocket>, mut write_stream: WriteHalf<TcpStream>, mut cipher: ChaCha20) {
        let mut buffer = [0u8; 512];
        loop {
            match read_stream.recv(&mut buffer).await {
                Ok(0) => break,
                Err(e) => { error!("Failed to read from stream: {}", e); break; }
                Ok(n) => {
                    cipher.apply_keystream(&mut buffer);
                    let _ = write_stream.write_all(&buffer[..n]).await;
                }
            }
        }
    }

    async fn read_write_t2u(mut read_stream: ReadHalf<TcpStream>, write_stream: Arc<UdpSocket>, mut cipher: ChaCha20) {
        let mut buffer = [0u8; 512];
        loop {
            match read_stream.read(&mut buffer).await {
                Ok(0) => break,
                Err(e) => { error!("Failed to read from stream: {}", e); break; }
                Ok(n) => {
                    cipher.apply_keystream(&mut buffer);
                    let _ = write_stream.send(&buffer[..n]).await;
                }
            }
        }
    }
}
