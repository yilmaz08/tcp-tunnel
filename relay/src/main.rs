use tokio::net::{TcpListener, TcpStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt, split, ReadHalf, WriteHalf};
use dotenvy::dotenv;
use std::env;

fn load_env() -> (String, u16, u16) {
    match dotenv() {
        Err(_) => panic!("dotenv couldn't be loaded!"),
        Ok(_) => println!("dotenv is loaded")
    }

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

    return (host, client_port.parse::<u16>().unwrap(), server_port.parse::<u16>().unwrap());
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

async fn handle_connection(server_stream: TcpStream, client_stream: TcpStream) {
    let (client_read, client_write) = split(client_stream);
    let (server_read, server_write) = split(server_stream);

    let client_to_server = read_write(client_read, server_write);
    let server_to_client = read_write(server_read, client_write);

    tokio::join!(client_to_server, server_to_client);
    println!("Connection completed!");
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let (host, client_port, server_port) = load_env();

    let server_listener = TcpListener::bind(format!("{}:{}", host, server_port)).await.unwrap();
    println!("Relay listening on {}:{} for server", host, server_port);
    let client_listener = TcpListener::bind(format!("{}:{}", host, client_port)).await.unwrap();
    println!("Relay listening on {}:{} for client", host, client_port);

    let (server_stream, _) = server_listener.accept().await.unwrap();
    println!("Server connected!");
    let (client_stream, _) = client_listener.accept().await.unwrap();

    println!("Connection established!");
    handle_connection(server_stream, client_stream).await;

    Ok(())
}
