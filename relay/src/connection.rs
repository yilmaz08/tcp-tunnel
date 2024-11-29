use tokio::io::{AsyncReadExt, AsyncWriteExt, split, ReadHalf, WriteHalf, AsyncBufReadExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;
use tokio::task;
use chacha20::ChaCha20;
use chacha20::cipher::{KeyIvInit, StreamCipher};
use base64::{Engine, engine::general_purpose};
use anyhow::Result;
use std::sync::Arc;
use std::net::SocketAddr;
use crate::environment::Environment;

pub struct Connection {
    pub index: u16,
    pub nonce: [u8; 12],
    pub env: Environment,
    pub server_listener: Arc<Mutex<TcpListener>>,
    pub client_listener: Arc<Mutex<TcpListener>>
}

impl Connection {
    pub fn new(index: u16, env: Environment, server_listener: Arc<Mutex<TcpListener>>, client_listener: Arc<Mutex<TcpListener>>) -> Self {
        println!("Connection with index {} constructed!", index);
        Self {
            nonce: crate::encryption::generate_random_nonce(),
            index,
            env,
            server_listener,
            client_listener
        }
    }

    pub async fn start(&mut self) -> Result<()> {
        println!("#{} Listening for server...", self.index);
        let (server_stream, _) = Connection::get_stream(self.server_listener.clone()).await;
        println!("#{} Server connected! Authenticating...", self.index);
        let mut server_stream = match self.server_connect(server_stream).await {
            Ok(val) => { println!("Authenticated!"); val },
            Err(e) => { println!("Drop: {:?}", e); return Ok(()); }
        };
        println!("#{} Listening for client...", self.index);
        let (client_stream, _) = Connection::get_stream(self.client_listener.clone()).await;
        println!("#{} Client connected! Starting data stream...", self.index);
        server_stream.write(&[1u8; 1]).await.unwrap(); // Send starting byte

        return self.start_data_stream(server_stream, client_stream).await;
    }

    async fn get_stream(listener_lock: Arc<Mutex<TcpListener>>) -> (TcpStream, SocketAddr) {
        let listener = listener_lock.lock().await;
        let result = listener.accept().await.unwrap();
        drop(listener);
        return result;
    }

    async fn server_connect(&mut self, mut stream: TcpStream) -> Result<TcpStream> {
        // Encode the Nonce
        let engine = general_purpose::STANDARD;
        let base64_nonce = engine.encode(self.nonce);
        // Send the Nonce
        stream.write(base64_nonce.as_bytes()).await?;
        stream.write(b"\r\n").await?;
        // Parse secret
        let mut cipher: ChaCha20 = ChaCha20::new(&self.env.secret.into(), &self.nonce.into());

        // Expect `encoded(encrypted("AUTH"))\r\n` for verification
        let mut reader = BufReader::new(&mut stream);
        let mut base64_enc_message = String::new();
        if reader.read_line(&mut base64_enc_message).await.unwrap() > 0 {
            let mut message: Vec<u8> = engine.decode(base64_enc_message.trim_end()).unwrap();
            cipher.apply_keystream(&mut message);
            if b"AUTH".to_vec() != message {
                return Err(anyhow::Error::msg("Unauthorized"));
            }
        } else {
            return Err(anyhow::Error::msg("Nothing received"));
        }
        return Ok(stream);
    }

    async fn start_data_stream(&mut self, server_stream: TcpStream, client_stream: TcpStream) -> Result<()> {
        let (client_read, client_write) = split(server_stream);
        let (server_read, server_write) = split(client_stream);

        let client_cipher = ChaCha20::new(&self.env.secret.into(), &self.nonce.into());
        let server_cipher = ChaCha20::new(&self.env.secret.into(), &self.nonce.into());

        let mut client_to_server = task::spawn(Self::read_write(client_read, server_write, client_cipher));
        let mut server_to_client = task::spawn(Self::read_write(server_read, client_write, server_cipher));

        tokio::select! {
            _ = &mut client_to_server => { 
                println!("Client to server ended!");
                server_to_client.abort();
            },
            _ = &mut server_to_client => { 
                println!("Server to client ended!"); 
                client_to_server.abort();
            }
        }

        println!("Connection completed!");
        Ok(())
    }

    async fn read_write(mut read_stream: ReadHalf<TcpStream>, mut write_stream: WriteHalf<TcpStream>, mut cipher: ChaCha20) {
        let mut buffer = [0u8; 512];
        loop {
            match read_stream.read(&mut buffer).await {
                Ok(0) => break,
                Err(e) => { println!("Failed to read from stream: {}", e); break; }
                Ok(n) => {
                    cipher.apply_keystream(&mut buffer);
                    let _ = write_stream.write_all(&buffer[..n]).await;
                }
            }
        }
    }
}
