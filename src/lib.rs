use chacha20::ChaCha20;
use tokio::net::TcpStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt, split, ReadHalf, WriteHalf, AsyncBufReadExt, BufReader};
use chacha20::cipher::{KeyIvInit, StreamCipher};
use log::{info, trace, error, debug};

pub mod encryption;

pub async fn read_write(mut read_stream: ReadHalf<TcpStream>, mut write_stream: WriteHalf<TcpStream>, mut cipher: ChaCha20) {
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
