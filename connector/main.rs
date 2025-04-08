use anyhow::Result;
use log::{debug, error, info};
use std::net::SocketAddr;
use tcp_tunnel::{tunnel::Tunnel, error::TunnelError};
use tokio::{
    net::TcpStream,
    task,
    time::{sleep, Duration},
};

mod environment;

const CONNREF_TIMEOUT: Duration = Duration::from_secs(5);
const SECRET_MISMATCH_TIMEOUT: Duration = Duration::from_secs(5);

async fn start_connection(
    log_target: &str,
    secret: [u8; 32],
    relay_addr: SocketAddr,
    server_addr: SocketAddr,
) {
    loop {
        debug!(target: log_target, "Connecting to relay...");
        let relay_stream = match TcpStream::connect(relay_addr).await {
            Ok(stream) => stream,
            Err(e) => {
                match e.kind() {
                    std::io::ErrorKind::ConnectionRefused => {
                        error!(target: log_target, "Connection refused: Sleeping for {:?}...", CONNREF_TIMEOUT);
                        sleep(CONNREF_TIMEOUT).await;
                    }
                    _ => error!(target: log_target, "Couldn't connect to relay: {}", e),
                }
                continue;
            }
        };
        info!(target: log_target, "Connected to relay!");

        let tunnel = match Tunnel::init(relay_stream, false, secret).await {
            Ok(tunnel) => tunnel,
            Err(e) => {
                match e.downcast_ref::<TunnelError>() {
                    Some(TunnelError::SecretMismatch) => {
                        error!(target: log_target, "{}: Sleeping for {:?}...", e, SECRET_MISMATCH_TIMEOUT);
                        sleep(SECRET_MISMATCH_TIMEOUT).await;
                    }
                    _ => error!(target: log_target, "Couldn't initialize a tunnel: {}", e),
                }
                continue;
            }
        };

        debug!(target: log_target, "Connecting to server...");
        let server_stream = match TcpStream::connect(server_addr).await {
            Ok(stream) => stream,
            Err(e) => {
                match e.kind() {
                    std::io::ErrorKind::ConnectionRefused => {
                        drop(tunnel);
                        error!(target: log_target, "Connection refused: Sleeping for {:?}...", CONNREF_TIMEOUT);
                        sleep(CONNREF_TIMEOUT).await;
                    }
                    _ => error!(target: log_target, "Couldn't connect to server: {}", e),
                }
                continue;
            }
        };
        info!(target: log_target, "Connected to server!");

        if let Err(e) = tunnel.run(server_stream).await {
            error!(target: log_target, "Tunnel failed: {}", e);
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let env = environment::Environment::new()?;

    env_logger::builder().filter_level(env.log_level).init();

    for index in 0..env.connections {
        task::spawn(async move {
            start_connection(
                &format!("conn #{}", index),
                env.secret,
                env.relay_addr,
                env.server_addr,
            )
            .await;
        });
    }

    std::thread::park();
    Ok(())
}
