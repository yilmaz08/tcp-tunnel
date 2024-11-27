use tokio::net::TcpStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt, split, ReadHalf, WriteHalf, AsyncBufReadExt, BufReader};
use chacha20::ChaCha20;
use chacha20::cipher::{KeyIvInit, StreamCipher};
use base64::{Engine, engine::general_purpose};
use anyhow::Result;
use crate::environment::Environment;

pub struct Connection {
    pub nonce: [u8; 12],
    pub env: Environment
}

impl Connection {
    pub fn new(env: Environment) -> Self {
        Self {
            nonce: [0x0; 12],
            env
        }
    }

    pub async fn start(&mut self) -> Result<()> {
        println!("Connecting to relay...");
        let relay_stream = Connection::create_stream(self.env.relay_host.clone(), self.env.relay_port.clone()).await;
        println!("Connected to relay! Authenticating...");
        let relay_stream = match self.relay_connect(relay_stream).await {
            Ok(val) => { println!("Authenticated!"); val },
            Err(e) => { println!("Drop: {:?}", e); return Ok(()); }
        };
        println!("Connecting to server...");
        let server_stream = Connection::create_stream(self.env.server_host.clone(), self.env.server_port.clone()).await;
        println!("Connected to server! Starting data stream...");
        return self.start_data_stream(relay_stream, server_stream).await;
    }

    async fn create_stream(host: String, port: u16) -> TcpStream {
        return TcpStream::connect(format!("{}:{}", host, port)).await.unwrap();
    }

    async fn start_data_stream(&mut self, relay_stream: TcpStream, server_stream: TcpStream) -> Result<()> {
        let (relay_read, relay_write) = split(relay_stream);
        let (server_read, server_write) = split(server_stream);

        let relay_cipher = ChaCha20::new(&self.env.secret.into(), &self.nonce.into());
        let server_cipher = ChaCha20::new(&self.env.secret.into(), &self.nonce.into());

        let relay_to_server = Connection::read_write(relay_read, server_write, relay_cipher);
        let server_to_relay = Connection::read_write(server_read, relay_write, server_cipher);

        tokio::join!(relay_to_server, server_to_relay);
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

    async fn relay_connect(&mut self, mut stream: TcpStream) -> Result<TcpStream> {
        // Get the Generated Nonce
        let mut reader = BufReader::new(&mut stream);

        // Read the first line
        let mut base64_nonce = String::new();
        if reader.read_line(&mut base64_nonce).await? > 0 {
            base64_nonce = base64_nonce.trim_end().to_string();
            println!("- Nonce exchange completed!");
        } else {
            panic!("- No nonce received.");
        }
        // Decode base64 encoded nonce
        let engine = general_purpose::STANDARD;
        self.nonce = engine.decode(base64_nonce).unwrap().try_into().unwrap();

        // Create new cipher
        let mut cipher = ChaCha20::new(&self.env.secret.into(), &self.nonce.into());

        // Send `encoded(encrypted("AUTH"))\r\n` for verification
        let mut message: Vec<u8> = b"AUTH".to_vec();
        cipher.apply_keystream(&mut message);
        let encoded_message = engine.encode(&message);
        stream.write(encoded_message.as_bytes()).await?;
        stream.write(b"\r\n").await?;

        return Ok(stream);
    }
}
