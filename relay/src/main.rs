use tokio::net::TcpListener;
use dotenvy::dotenv;
use std::env;
use tokio::io::{AsyncBufReadExt, BufReader};

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

async fn handle_client(host: &String, port: &u16) {
    let listener = TcpListener::bind(format!("{}:{}", host, port)).await.unwrap();
    println!("Relay listening on {}:{} for client", host, port);
    loop {
        let (stream, _) = listener.accept().await.unwrap();

        let reader = BufReader::new(stream);
        let mut lines = reader.lines();

        while let Some(line) = lines.next_line().await.unwrap() {
            println!("Received from client: {}", line);
        }

        println!("Connection closed.");
    }
}

async fn handle_server(host: &String, port: &u16) {
    let listener = TcpListener::bind(format!("{}:{}", host, port)).await.unwrap();
    println!("Relay listening on {}:{} for server", host, port);
    loop {
        let (stream, _) = listener.accept().await.unwrap();

        let reader = BufReader::new(stream);
        let mut lines = reader.lines();

        while let Some(line) = lines.next_line().await.unwrap() {
            println!("Received from server: {}", line);
        }

        println!("Connection closed.");
    }
}

#[tokio::main]
async fn main() {
    let (host, client_port, server_port) = load_env();

    let client_func = handle_client(&host, &client_port);
    let server_func = handle_server(&host, &server_port);

    tokio::join!(server_func, client_func);
}
