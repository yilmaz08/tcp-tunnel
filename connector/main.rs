use anyhow::Result;
use log::{debug, error, info};
use std::net::SocketAddr;
use tcp_tunnel::tunnel::Tunnel;
use tokio::{net::TcpStream, runtime::Runtime};

mod environment;

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
                error!(target: log_target, "Couldn't connect to relay: {}", e);
                continue;
            }
        };
        info!(target: log_target, "Connected to relay!");

        let tunnel = match Tunnel::init(relay_stream, false, secret).await {
            Ok(tunnel) => tunnel,
            Err(e) => {
                error!(target: log_target, "Couldn't initialize a tunnel: {}", e);
                continue;
            }
        };

        debug!(target: log_target, "Connecting to server...");
        let server_stream = match TcpStream::connect(server_addr).await {
            Ok(stream) => stream,
            Err(e) => {
                error!(target: log_target, "Couldn't connect to server: {}", e);
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

    let rt = Runtime::new()?;

    for index in 0..env.connections {
        rt.spawn(async move {
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
