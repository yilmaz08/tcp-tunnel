use tokio::net::TcpListener;
use tokio::sync::Mutex;
use std::sync::Arc;

mod encryption;
mod environment;
mod connection;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let env: environment::Environment = environment::Environment::new();

    let server_listener: Arc<Mutex<TcpListener>> = Arc::new(Mutex::new(TcpListener::bind(format!("{}:{}", env.host, env.server_port)).await.unwrap()));
    println!("Server listener is set up on {}:{}", env.host, env.server_port);
    let client_listener: Arc<Mutex<TcpListener>> = Arc::new(Mutex::new(TcpListener::bind(format!("{}:{}", env.host, env.client_port)).await.unwrap()));
    println!("Client listener is set up on {}:{}", env.host, env.client_port);

    let mut conn: connection::Connection = connection::Connection::new(env.clone(), server_listener.clone(), client_listener.clone());

    return conn.start().await;
}
