use crate::config::{ConnectionType, Direction, Endpoint};
use crate::tunnel::Tunnel;
use crate::error::TunnelError;
use crate::encryption::generate_secret_from_string;
use anyhow::Result;
use log::{debug, error, info};
use std::{net::SocketAddr, sync::Arc};
use tokio::{
    io::split,
    net::{TcpListener, TcpStream},
    sync::Mutex,
    time::{sleep, Duration},
};

#[derive(Debug, Clone)]
pub enum ConnectionData {
    Inbound {
        listener_lock: Arc<Mutex<TcpListener>>,
        secret_option: Option<[u8; 32]>,
    },
    Outbound {
        addr: SocketAddr,
        secret_option: Option<[u8; 32]>,
    },
}

pub enum Connection {
    Tunnel(Tunnel),
    Direct(TcpStream),
}

// Gets endpoint and returns ConnectionData
pub async fn get_connection_data(endpoint: &Endpoint) -> Result<ConnectionData> {
    let addr = SocketAddr::new(endpoint.ip.unwrap_or("0.0.0.0".parse()?), endpoint.port);
    let secret_option = match endpoint.kind {
        ConnectionType::Tunnel => {
            match &endpoint.secret {
                Some(secret) => Some(generate_secret_from_string(secret.to_owned())),
                None => return Err(TunnelError::NoSecret.into())
            }
        }
        ConnectionType::Direct => None,
    };
    Ok(match endpoint.direction {
        Direction::Outbound => ConnectionData::Outbound {
            addr,
            secret_option,
        },
        Direction::Inbound => ConnectionData::Inbound {
            listener_lock: Arc::new(Mutex::new(TcpListener::bind(addr).await?)),
            secret_option,
        },
    })
}

// Gets ConnectionData and returns Connection
pub async fn connect(
    data: &ConnectionData,
    log_target: &str,
    endpoint_name: &str,
) -> Result<Connection> {
    Ok(match &data {
        ConnectionData::Inbound {
            listener_lock,
            secret_option,
        } => {
            info!(target: log_target, "Listening for '{}'", endpoint_name);
            let listener = listener_lock.lock().await;
            let (stream, _) = listener.accept().await?;
            drop(listener);

            let conn = if let Some(secret) = secret_option {
                debug!(target: log_target, "Initializing the tunnel");
                Connection::Tunnel(Tunnel::init(stream, true, *secret).await?)
            } else {
                Connection::Direct(stream)
            };

            debug!(target: log_target, "Connection from '{}'", endpoint_name);
            conn
        }
        ConnectionData::Outbound {
            addr,
            secret_option,
        } => {
            info!(target: log_target, "Connecting to '{}'", endpoint_name);
            let stream = TcpStream::connect(addr).await?;

            let conn = if let Some(secret) = secret_option {
                debug!(target: log_target, "Initializing the tunnel");
                Connection::Tunnel(Tunnel::init(stream, false, *secret).await?)
            } else {
                Connection::Direct(stream)
            };

            debug!(target: log_target, "Connected to '{}'", endpoint_name);
            conn
        }
    })
}

pub async fn start_connection(a: ConnectionData, b: ConnectionData, log_target: &str) {
    loop {
        let ts_a = match connect(&a, log_target, "Stream A").await {
            Ok(x) => x,
            Err(e) => {
                if let Some(io_error) = e.downcast_ref::<std::io::Error>() {
                    if io_error.kind() == std::io::ErrorKind::ConnectionRefused {
                        error!(target: log_target, "Connection refused! Sleeping for 5 seconds...");
                        sleep(Duration::from_secs(5)).await;
                        continue;
                    }
                }
                error!(target: log_target, "Couldn't get stream B: {}", e);
                continue;
            }
        };

        let ts_b = match connect(&b, log_target, "Stream B").await {
            Ok(x) => x,
            Err(e) => {
                if let Some(io_error) = e.downcast_ref::<std::io::Error>() {
                    if io_error.kind() == std::io::ErrorKind::ConnectionRefused {
                        drop(ts_a);
                        error!(target: log_target, "Connection refused! Sleeping for 5 seconds...");
                        sleep(Duration::from_secs(5)).await;
                        continue;
                    }
                }
                error!(target: log_target, "Couldn't get stream B: {}", e);
                continue;
            }
        };

        let res = match (ts_a, ts_b) {
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
            (Connection::Tunnel(a), Connection::Tunnel(b)) => a.join(b).await,

            (Connection::Tunnel(a), Connection::Direct(b)) => a.run(b).await,
            (Connection::Direct(a), Connection::Tunnel(b)) => b.run(a).await,
        };

        if let Err(e) = res {
            error!(target: log_target, "Route failed: {}", e);
        }
    }
}
