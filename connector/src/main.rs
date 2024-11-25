use tokio::net::TcpStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt, split, ReadHalf, WriteHalf};
use dotenvy::dotenv;
use std::env;
use sha2::{Sha256, Digest};
use chacha20::ChaCha20;
use chacha20::cipher::{KeyIvInit, StreamCipher};

fn generate_secret_from_string(secret_str: String) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(secret_str);
    let mut secret = [0u8; 32];
    secret.copy_from_slice(&hasher.finalize());
    secret
}

fn load_env() -> (String, String, u16, u16, String) {
    match dotenv() {
        Err(_) => panic!("dotenv couldn't be loaded!"),
        Ok(_) => println!("dotenv is loaded")
    }

    let secret = match env::var("SECRET") {
        Ok(val) => val,
        Err(_) => panic!("no SECRET found")
    };
    let relay_host = match env::var("RELAY_HOST") {
        Ok(val) => val,
        Err(_) => panic!("couldn't find RELAY_HOST in dotenv")
    };
    let server_host = match env::var("SERVER_HOST") {
        Ok(val) => val,
        Err(_) => panic!("couldn't find SERVER_HOST in dotenv")
    };
    let relay_port = match env::var("RELAY_PORT") {
        Ok(val) => val,
        Err(_) => panic!("couldn't find RELAY_PORT in dotenv")
    };
    let server_port = match env::var("SERVER_PORT") {
        Ok(val) => val,
        Err(_) => panic!("couldn't find SERVER_PORT in dotenv")
    };

    return (relay_host, server_host, relay_port.parse::<u16>().unwrap(), server_port.parse::<u16>().unwrap(), secret);
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

async fn handle_connection(relay_stream: TcpStream, server_stream: TcpStream, secret: [u8; 32], nonce: [u8; 12]) {
    let (relay_read, relay_write) = split(relay_stream);
    let (server_read, server_write) = split(server_stream);

    let relay_cipher = ChaCha20::new(&secret.into(), &nonce.into());
    let server_cipher = ChaCha20::new(&secret.into(), &nonce.into());

    let relay_to_server = read_write(relay_read, server_write, relay_cipher);
    let server_to_relay = read_write(server_read, relay_write, server_cipher);

    tokio::join!(relay_to_server, server_to_relay);
    println!("Connection completed!");
}

async fn relay_connect(host: String, port: u16, secret: String) -> std::io::Result<(TcpStream, [u8; 32], [u8; 12])> {
    let stream = TcpStream::connect(format!("{}:{}", host, port)).await?;
    
    // Get the Generated Nonce - TODO
    let nonce = [0x0; 12]; // Temporarily Static
    
    // Parse secret
    let secret = generate_secret_from_string(secret);

    return Ok((stream, secret, nonce));
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let (relay_host, server_host, relay_port, server_port, secret_str) = load_env();

    let (relay_stream, secret, nonce) = relay_connect(relay_host.clone(), relay_port, secret_str).await.unwrap();
    println!("Connector connected to relay on {}:{}", relay_host, relay_port);
    let server_stream = TcpStream::connect(format!("{}:{}", server_host, server_port)).await?;
    println!("Connector connected to server on {}:{}", server_host, server_port);

    println!("Connection established!");
    handle_connection(relay_stream, server_stream, secret, nonce).await;

    Ok(())
}
