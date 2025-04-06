use anyhow::Result;
use log::{debug, error, info};
use std::sync::Arc;
use tcp_tunnel::tunnel::Tunnel;
use tokio::{
    net::{TcpListener, TcpStream},
    runtime::Runtime,
    sync::Mutex,
};

mod environment;

async fn get_stream(
    listener_lock: &Arc<Mutex<TcpListener>>,
) -> Result<(TcpStream, std::net::SocketAddr)> {
    let listener = listener_lock.lock().await;
    let (stream, addr) = listener.accept().await?;
    drop(listener);
    Ok((stream, addr))
}

async fn start_connection(
    server_listener: Arc<Mutex<TcpListener>>,
    client_listener: Arc<Mutex<TcpListener>>,
    log_target: &str,
    secret: [u8; 32],
) {
    loop {
        debug!(target: log_target, "Listening for server...");
        let server_stream = match get_stream(&server_listener).await {
            Ok((stream, _)) => stream,
            Err(e) => {
                error!(target: log_target, "Couldn't get server stream: {}", e);
                continue;
            }
        };
        info!(target: log_target, "Server connected!");

        let tunnel = match Tunnel::init(server_stream, true, secret).await {
            Ok(tunnel) => tunnel,
            Err(e) => {
                error!(target: log_target, "Couldn't initialize a tunnel: {}", e);
                continue;
            }
        };

        debug!(target: &log_target, "Listening for client...");
        let client_stream = match get_stream(&client_listener).await {
            Ok((stream, _)) => stream,
            Err(e) => {
                error!(target: log_target, "Couldn't get client stream: {}", e);
                continue;
            }
        };
        info!(target: log_target, "Client connected!");

        if let Err(e) = tunnel.run(client_stream).await {
            error!(target: log_target, "Tunnel failed: {}", e);
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let env = environment::Environment::new()?;

    env_logger::builder().filter_level(env.log_level).init();

    let server_listener = Arc::new(Mutex::new(TcpListener::bind(env.server_addr).await?));
    info!("Server listener is set up on {}", env.server_addr);
    let client_listener = Arc::new(Mutex::new(TcpListener::bind(env.client_addr).await?));
    info!("Client listener is set up on {}", env.client_addr);

    let rt = Runtime::new()?;

    for index in 0..env.connections {
        rt.spawn({
            let server_listener = server_listener.clone();
            let client_listener = client_listener.clone();
            async move {
                start_connection(
                    server_listener,
                    client_listener,
                    &format!("conn #{}", index),
                    env.secret,
                )
                .await;
            }
        });
    }

    std::thread::park();
    Ok(())
}
