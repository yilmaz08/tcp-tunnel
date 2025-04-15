use crate::config::{ConnectionType, Direction, Endpoint};
use crate::tunnel::Tunnel;
use anyhow::Result;
use log::{debug, error, info};
use std::{net::SocketAddr, sync::Arc};
use tokio::io::split;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;

#[derive(Debug, Clone)]
pub enum ConnectionData {
    Inbound {
        listener_lock: Arc<Mutex<TcpListener>>,
        secret_option: Option<[u8; 32]>,
    },
    Outbound {
        addr: SocketAddr,
        secret_option: Option<[u8; 32]>,
    }
}

pub enum Connection {
    Tunnel(Tunnel),
    Direct(TcpStream),
}

// Gets endpoint and returns ConnectionData
pub async fn get_connection_data(endpoint: &Endpoint, secret: [u8; 32]) -> Result<ConnectionData> {
    let addr = SocketAddr::new(endpoint.ip.unwrap_or("0.0.0.0".parse()?), endpoint.port);
    let secret_option = match endpoint.kind {
        ConnectionType::Tunnel => Some(secret),
        ConnectionType::Direct => None,
    };
    Ok(match endpoint.direction {
        Direction::Outbound => ConnectionData::Outbound { addr, secret_option },
        Direction::Inbound => {
            ConnectionData::Inbound {
                listener_lock: Arc::new(Mutex::new(TcpListener::bind(addr).await?)),
                secret_option
            }
        }
    })
}

// Gets ConnectionData and returns Connection
pub async fn connect(data: &ConnectionData) -> Result<Connection> {
    Ok(match &data {
        ConnectionData::Inbound { listener_lock, secret_option } => {
            let listener = listener_lock.lock().await;
            let (stream, _) = listener.accept().await?;
            drop(listener);
            if let Some(secret) = secret_option {
                Connection::Tunnel(Tunnel::init(stream, true, *secret).await?)
            } else {
                Connection::Direct(stream)
            }
        }
        ConnectionData::Outbound { addr, secret_option } => {
            let stream = TcpStream::connect(addr).await?;
            if let Some(secret) = secret_option {
                Connection::Tunnel(Tunnel::init(stream, false, *secret).await?)
            } else {
                Connection::Direct(stream)
            }
        }
    })
}

pub async fn start_connection(a: ConnectionData, b: ConnectionData, log_target: &str) {
    loop {
        info!(target: log_target, "Connecting to stream A...");
        let ts_a = match connect(&a).await {
            Ok(x) => x,
            Err(e) => {
                error!(target: log_target, "Couldn't get stream A: {:#?}", e);
                continue;
            }
        };
        debug!(target: log_target, "Connected to stream A!");
        info!(target: log_target, "Connecting to stream B...");
        let ts_b = match connect(&b).await {
            Ok(x) => x,
            Err(e) => {
                error!(target: log_target, "Couldn't get stream B: {}", e);
                continue;
            }
        };
        debug!(target: log_target, "Connected to stream B!");

        let res = match (ts_a, ts_b) {
            (Connection::Tunnel(a), Connection::Tunnel(b)) => a.join(b).await,
            (Connection::Direct(a), Connection::Direct(b)) => {
                let (a_read, a_write) = split(a);
                let (b_read, b_write) = split(b);

                let mut a_to_b = tokio::task::spawn(Tunnel::read_write(a_read, b_write, vec![]));
                let mut b_to_a = tokio::task::spawn(Tunnel::read_write(b_read, a_write, vec![]));

                tokio::select! {
                    _ = &mut a_to_b => b_to_a.abort(),
                    _ = &mut b_to_a => a_to_b.abort()
                }

                Ok(())
            }

            (Connection::Tunnel(a), Connection::Direct(b)) => a.run(b).await,
            (Connection::Direct(a), Connection::Tunnel(b)) => b.run(a).await,
        };

        if let Err(e) = res {
            error!(target: log_target, "Failed: {}", e);
        }
    }
}
