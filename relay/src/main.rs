use chacha20::ChaCha20;
use chacha20::cipher::{KeyIvInit, StreamCipher};
use tokio::net::{TcpListener, TcpStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt, split, ReadHalf, WriteHalf, AsyncBufReadExt, BufReader};
use base64::{Engine, engine::general_purpose};

mod encryption;
mod environment;

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

async fn handle_connection(server_stream: TcpStream, client_stream: TcpStream, secret: [u8; 32], nonce: [u8; 12]) {
    let (client_read, client_write) = split(client_stream);
    let (server_read, server_write) = split(server_stream);

    let client_cipher = ChaCha20::new(&secret.into(), &nonce.into());
    let server_cipher = ChaCha20::new(&secret.into(), &nonce.into());

    let client_to_server = read_write(client_read, server_write, client_cipher);
    let server_to_client = read_write(server_read, client_write, server_cipher);

    tokio::join!(client_to_server, server_to_client);
    println!("Connection completed!");
}

async fn server_connect(listener: TcpListener, secret: [u8; 32]) -> std::io::Result<(TcpStream, [u8; 12])> {
    let (mut stream, _) = listener.accept().await.unwrap();
    
    // Generate Random Nonce
    let nonce: [u8; 12] = encryption::generate_random_nonce();
    // Encode the Nonce
    let engine = general_purpose::STANDARD;
    let base64_nonce = engine.encode(nonce);
    // Send the Nonce
    stream.write(base64_nonce.as_bytes()).await?;
    stream.write(b"\r\n").await?;
    // Parse secret
    let mut cipher: ChaCha20 = ChaCha20::new(&secret.into(), &nonce.into());

    // Expect `encoded(encrypted("AUTH"))\r\n` for verification
    let mut reader = BufReader::new(&mut stream);
    let mut base64_enc_message = String::new();
    if reader.read_line(&mut base64_enc_message).await? > 0 {
        let engine = general_purpose::STANDARD;
        let mut message: Vec<u8> = engine.decode(base64_enc_message.trim_end()).unwrap();
        cipher.apply_keystream(&mut message);
        if b"AUTH".to_vec() != message {
            panic!("Unauthorized");
        }

    } else {
        panic!("Nothing received");
    }

    return Ok((stream, nonce));
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let env: environment::Environment = environment::Environment::new();

    let server_listener = TcpListener::bind(format!("{}:{}", env.host, env.server_port)).await.unwrap();
    println!("Relay listening on {}:{} for server", env.host, env.server_port);
    let client_listener = TcpListener::bind(format!("{}:{}", env.host, env.client_port)).await.unwrap();
    println!("Relay listening on {}:{} for client", env.host, env.client_port);

    let (server_stream, nonce) = server_connect(server_listener, env.secret).await.unwrap();
    println!("Server connected!");
    let (client_stream, _) = client_listener.accept().await.unwrap();

    println!("Connection established!");
    handle_connection(server_stream, client_stream, env.secret, nonce).await;

    Ok(())
}
