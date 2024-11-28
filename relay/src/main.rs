use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tokio::runtime::Runtime;
use std::sync::Arc;

mod encryption;
mod environment;
mod connection;

#[tokio::main]
async fn main() {
    let env: environment::Environment = environment::Environment::new();

    let server_listener: Arc<Mutex<TcpListener>> = Arc::new(Mutex::new(TcpListener::bind(format!("{}:{}", env.host, env.server_port)).await.unwrap()));
    println!("Server listener is set up on {}:{}", env.host, env.server_port);
    let client_listener: Arc<Mutex<TcpListener>> = Arc::new(Mutex::new(TcpListener::bind(format!("{}:{}", env.host, env.client_port)).await.unwrap()));
    println!("Client listener is set up on {}:{}", env.host, env.client_port);

    let rt = Runtime::new().unwrap();

    for index in 0..env.connections {
        let env = env.clone();
        let server_listener = server_listener.clone();
        let client_listener = client_listener.clone();
        rt.spawn(async move {
            loop {
                let mut conn: connection::Connection = connection::Connection::new(index, env.clone(), server_listener.clone(), client_listener.clone());
                let _ = conn.start().await;
                println!("#{:?} Ended", index);
            }
        });
    }

    std::thread::park();
}
