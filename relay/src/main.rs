use tokio::net::{TcpListener, TcpStream};
use dotenvy::dotenv;
use std::env;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Mutex;
use std::sync::Arc;

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

async fn handle_connection(server_stream: Arc<Mutex<Option<TcpStream>>>, client_stream: Arc<Mutex<Option<TcpStream>>>) {
    println!("Handling connections...");
    let mut server_stream_lock = server_stream.lock().await;
    let mut client_stream_lock = client_stream.lock().await;

    if let Some(ref mut server_conn) = *server_stream_lock {
        println!("Server connection established");
        let mut buffer = [0u8; 512];

        while let Ok(bytes_read) = server_conn.read(&mut buffer).await {
            if bytes_read == 0 { break; }
            println!("Received {} bytes from server: {:?}", bytes_read, &buffer[..bytes_read]);
            if let Some(ref mut client_conn) = *client_stream_lock {
                let _ = client_conn.write_all(&buffer[..bytes_read]).await;
            }
        }

        println!("Connection closed by server.");
    }
}

async fn server_listener_process(listener: TcpListener, server_stream: Arc<Mutex<Option<TcpStream>>>, client_stream: Arc<Mutex<Option<TcpStream>>>) {
    let (stream, _) = listener.accept().await.unwrap();
    {
        let mut stream_lock = server_stream.lock().await;
        *stream_lock = Some(stream);
    }
    handle_connection(server_stream, client_stream).await;
}

async fn client_listener_process(listener: TcpListener, server_stream: Arc<Mutex<Option<TcpStream>>>, client_stream: Arc<Mutex<Option<TcpStream>>>) {
    let (stream, _) = listener.accept().await.unwrap();
    {
        let mut stream_lock = client_stream.lock().await;
        *stream_lock = Some(stream);
    }
    handle_connection(server_stream, client_stream).await;
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let (host, client_port, server_port) = load_env();

    let server_listener = TcpListener::bind(format!("{}:{}", host, server_port)).await.unwrap();
    println!("Relay listening on {}:{} for server", host, server_port);
    let client_listener = TcpListener::bind(format!("{}:{}", host, client_port)).await.unwrap();
    println!("Relay listening on {}:{} for client", host, client_port);

    let server_stream: Arc<Mutex<Option<TcpStream>>> = Arc::new(Mutex::new(None));
    let client_stream: Arc<Mutex<Option<TcpStream>>> = Arc::new(Mutex::new(None));

    let server_func = server_listener_process(server_listener, server_stream.clone(), client_stream.clone());
    let client_func = client_listener_process(client_listener, server_stream.clone(), client_stream.clone());

    let _ = tokio::join!(server_func, client_func);

    Ok(())
}
