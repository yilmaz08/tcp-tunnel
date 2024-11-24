use tokio::net::{TcpListener, TcpStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt, split, ReadHalf, WriteHalf};
use dotenvy::dotenv;
use std::env;

fn load_env() -> (String, String, u16, u16) {
    match dotenv() {
        Err(_) => panic!("dotenv couldn't be loaded!"),
        Ok(_) => println!("dotenv is loaded")
    }

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

    return (relay_host, server_host, relay_port.parse::<u16>().unwrap(), server_port.parse::<u16>().unwrap());
}

async fn read_write(mut read_stream: ReadHalf<TcpStream>, mut write_stream: WriteHalf<TcpStream>) {
    let mut buffer = [0u8; 512];
    loop {
        match read_stream.read(&mut buffer).await {
            Ok(0) => break,
            Err(e) => { println!("Failed to read from stream: {}", e); break; }
            Ok(n) => {
                let _ = write_stream.write_all(&buffer[..n]).await;
            }
        }
    }
}

async fn handle_connection(relay_stream: TcpStream, server_stream: TcpStream) {
    let (relay_read, relay_write) = split(relay_stream);
    let (server_read, server_write) = split(server_stream);

    let relay_to_server = read_write(relay_read, server_write);
    let server_to_relay = read_write(server_read, relay_write);

    tokio::join!(relay_to_server, server_to_relay);
    println!("Connection completed!");
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let (relay_host, server_host, relay_port, server_port) = load_env();

    let relay_stream = TcpStream::connect(format!("{}:{}", relay_host, relay_port)).await?;
    println!("Connector connected to relay on {}:{}", relay_host, relay_port);
    let server_stream = TcpStream::connect(format!("{}:{}", server_host, server_port)).await?;
    println!("Connector connected to server on {}:{}", server_host, server_port);

    println!("Connection established!");
    handle_connection(relay_stream, server_stream).await;

    Ok(())
}
