use anyhow::Result;
use log::{debug, error, info, trace};
use std::{collections::HashMap, net::IpAddr, sync::Arc};
use tcp_tunnel::{error::TunnelError, tunnel::Tunnel};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::Mutex,
    task,
    time::{Instant, Duration},
};

const BAN_LENGTH: Duration = Duration::from_secs(5 * 60);

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
    ban_list: Arc<Mutex<HashMap<IpAddr, Instant>>>,
    log_target: &str,
    secret: [u8; 32],
) {
    loop {
        debug!(target: log_target, "Listening for server...");
        let (server_stream, server_addr) = match get_stream(&server_listener).await {
            Ok((stream, addr)) => {
                if let Some(&time) = ban_list.lock().await.get(&addr.ip()) {
                    if time > Instant::now() {
                        trace!(target: log_target, "Connection attempt from banned IP: {}", addr.ip());
                        continue;
                    }
                }
                (stream, addr)
            }
            Err(e) => {
                error!(target: log_target, "Couldn't get server stream: {}", e);
                continue;
            }
        };
        info!(target: log_target, "Server connected!");

        let tunnel = match Tunnel::init(server_stream, true, secret).await {
            Ok(tunnel) => tunnel,
            Err(e) => {
                match e.downcast_ref::<TunnelError>() {
                    Some(TunnelError::SecretMismatch | TunnelError::Timeout) => {
                        ban_list
                            .lock()
                            .await
                            .insert(server_addr.ip(), Instant::now() + BAN_LENGTH);
                        error!(target: log_target, "{}: {} is temporarily banned for {:?}", e, server_addr.ip(), BAN_LENGTH);
                    }
                    _ => error!(target: log_target, "Couldn't initialize a tunnel: {}", e),
                }
                continue;
            }
        };

        debug!(target: log_target, "Listening for client...");
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

    let ban_list = Arc::new(Mutex::new(HashMap::<IpAddr, Instant>::new()));

    for index in 0..env.connections {
        task::spawn({
            let server_listener = server_listener.clone();
            let client_listener = client_listener.clone();
            let ban_list = ban_list.clone();
            async move {
                start_connection(
                    server_listener,
                    client_listener,
                    ban_list,
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
