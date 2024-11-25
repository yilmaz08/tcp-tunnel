use chacha20::ChaCha20;
use chacha20::cipher::{KeyIvInit, StreamCipher};
use tokio::net::{TcpListener, TcpStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt, split, ReadHalf, WriteHalf};
use dotenvy::dotenv;
use std::env;
use rand::Rng;
use sha2::{Sha256, Digest};
use base64::{Engine, engine::general_purpose};

fn generate_random_nonce() -> [u8; 12] {
    let mut rng = rand::thread_rng();
    let mut nonce = [0u8; 12];
    rng.fill(&mut nonce);
    nonce
}

fn generate_secret_from_string(secret_str: String) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(secret_str);
    let mut secret = [0u8; 32];
    secret.copy_from_slice(&hasher.finalize());
    secret
}

fn load_env() -> (String, u16, u16, String) {
    match dotenv() {
        Err(_) => panic!("dotenv couldn't be loaded!"),
        Ok(_) => println!("dotenv is loaded")
    }

    let secret = match env::var("SECRET") {
        Ok(val) => val,
        Err(_) => panic!("no SECRET found")
    };
    let host = match env::var("HOST") {
        Ok(val) => val,
        Err(_) => panic!("couldn't find HOST in dotenv")
    };
    let client_port = match env::var("CLIENT_PORT") {
        Ok(val) => val,
        Err(_) => panic!("couldn't find CLIENT_PORT in dotenv")
    };
    let server_port = match env::var("SERVER_PORT") {
        Ok(val) => val,
        Err(_) => panic!("couldn't find SERVER_PORT in dotenv")
    };

    return (host, client_port.parse::<u16>().unwrap(), server_port.parse::<u16>().unwrap(), secret);
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

async fn server_connect(listener: TcpListener, secret: String) -> std::io::Result<(TcpStream, [u8; 32], [u8; 12])> {
    let (mut stream, _) = listener.accept().await.unwrap();
    
    // Generate Random Nonce
    let nonce: [u8; 12] = generate_random_nonce();
    // Encode the Nonce
    let engine = general_purpose::STANDARD;
    let base64_nonce = engine.encode(nonce);
    // Send the Nonce
    stream.write(base64_nonce.as_bytes()).await?;
    stream.write(b"\r\n").await?;
    // Parse secret
    let secret: [u8; 32] = generate_secret_from_string(secret);

    return Ok((stream, secret, nonce));
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let (host, client_port, server_port, secret) = load_env();

    let server_listener = TcpListener::bind(format!("{}:{}", host, server_port)).await.unwrap();
    println!("Relay listening on {}:{} for server", host, server_port);
    let client_listener = TcpListener::bind(format!("{}:{}", host, client_port)).await.unwrap();
    println!("Relay listening on {}:{} for client", host, client_port);

    let (server_stream, secret, nonce) = server_connect(server_listener, secret).await.unwrap();
    println!("Server connected!");
    let (client_stream, _) = client_listener.accept().await.unwrap();

    println!("Connection established!");
    handle_connection(server_stream, client_stream, secret, nonce).await;

    Ok(())
}
