use crate::{
    config::{ConnectionType, Direction, Endpoint},
    encryption::generate_secret_from_string,
    error::{ConfigError, TunnelError},
    tunnel::Tunnel,
};
use anyhow::{anyhow, Result};
use dashmap::DashMap;
use log::{debug, error, info};
use std::{
    net::{IpAddr, SocketAddr, ToSocketAddrs},
    sync::Arc,
};
use tokio::{
    net::{TcpListener, TcpStream},
    time::{sleep, Duration, Instant},
};

const CONNREF_TIMEOUT: Duration = Duration::from_secs(5);
const SECRET_REJECTED_TIMEOUT: Duration = Duration::from_secs(30);
const NONCE_EARLY_EOF_TIMEOUT: Duration = Duration::from_secs(15);
const BAN_LENGTH: Duration = Duration::from_secs(60 * 5);

#[derive(Clone)]
pub enum ConnectionData {
    Inbound {
        listener: Arc<TcpListener>,
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
    let addr_str = format!("{}:{}", endpoint.host.clone().unwrap_or("0.0.0.0".to_owned()), endpoint.port);
    let addr = match addr_str.to_socket_addrs()?.next() {
        Some(a) => a,
        None => return Err(anyhow!("Couldn't resolve address!"))
    };

    let secret_option = match endpoint.kind {
        ConnectionType::Tunnel => match &endpoint.secret {
            Some(secret) => Some(generate_secret_from_string(secret.to_owned())),
            None => return Err(ConfigError::NoSecret.into()),
        },
        ConnectionType::Direct => None,
    };

    Ok(match endpoint.direction {
        Direction::Outbound => ConnectionData::Outbound {
            addr,
            secret_option,
        },
        Direction::Inbound => ConnectionData::Inbound {
            listener: Arc::new(TcpListener::bind(addr).await?),
            secret_option,
        },
    })
}

// Gets ConnectionData and returns Connection
pub async fn connect(
    data: &ConnectionData,
    ban_list: &DashMap<IpAddr, Instant>,
    log_target: &str,
    endpoint_name: &str,
) -> Result<Connection> {
    Ok(match &data {
        ConnectionData::Inbound {
            listener,
            secret_option,
        } => {
            info!(target: log_target, "Listening for '{}'", endpoint_name);

            let (stream, addr) = listener.accept().await?;

            let conn = match secret_option {
                Some(secret) => {
                    if let Some(time) = ban_list.get(&addr.ip()) {
                        if *time > Instant::now() {
                            return Err(TunnelError::ConnAttemptFromBannedIP.into());
                        }
                    }

                    debug!(target: log_target, "Initializing the tunnel");
                    Connection::Tunnel(Tunnel::init(stream, true, *secret).await?)
                }
                None => Connection::Direct(stream),
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

            let conn = match secret_option {
                Some(secret) => {
                    debug!(target: log_target, "Initializing the tunnel");
                    Connection::Tunnel(Tunnel::init(stream, false, *secret).await?)
                }
                None => Connection::Direct(stream),
            };

            debug!(target: log_target, "Connected to '{}'", endpoint_name);
            conn
        }
    })
}

// Handle error for the function connect
async fn handle_connection_error(
    error: anyhow::Error,
    ban_list: &DashMap<IpAddr, Instant>,
    log_target: &str,
    endpoint_name: &str,
) {
    if let Some(io_error) = error.downcast_ref::<std::io::Error>() {
        if io_error.kind() == std::io::ErrorKind::ConnectionRefused {
            error!(target: log_target, "Connection refused! Sleeping for {:?}...", CONNREF_TIMEOUT);
            sleep(CONNREF_TIMEOUT).await;
            return;
        }
    } else if let Some(tunnel_error) = error.downcast_ref::<TunnelError>() {
        match tunnel_error {
            TunnelError::SecretRejected => {
                error!(target: log_target, "{}: Sleeping for {:?}...", error, SECRET_REJECTED_TIMEOUT);
                sleep(SECRET_REJECTED_TIMEOUT).await;
                return;
            }
            TunnelError::NonceEarlyEOF => {
                error!(target: log_target, "{}: Sleeping for {:?}...", error, NONCE_EARLY_EOF_TIMEOUT);
                sleep(NONCE_EARLY_EOF_TIMEOUT).await;
                return;
            }
            TunnelError::SecretMismatch(addr) | TunnelError::Timeout(addr) => {
                ban_list.insert(*addr, Instant::now() + BAN_LENGTH);
                info!(target: log_target, "{}: {} is banned for {:?}", error, addr, BAN_LENGTH);
                return;
            }
            _ => {}
        }
    }

    error!(target: log_target, "Connection '{}' failed: {}", endpoint_name, error);
}

pub async fn route(
    endpoint_a: ConnectionData,
    endpoint_b: ConnectionData,
    ban_list: DashMap<IpAddr, Instant>,
    log_target: &str,
) {
    loop {
        let conn_a = match connect(&endpoint_a, &ban_list, log_target, "A").await {
            Ok(conn) => conn,
            Err(e) => {
                handle_connection_error(e, &ban_list, log_target, "A").await;
                continue;
            }
        };
        let conn_b = match connect(&endpoint_b, &ban_list, log_target, "B").await {
            Ok(conn) => conn,
            Err(e) => {
                drop(conn_a);
                handle_connection_error(e, &ban_list, log_target, "B").await;
                continue;
            }
        };

        let result = match (conn_a, conn_b) {
            (Connection::Direct(a), Connection::Direct(b)) => Tunnel::proxy(a, b).await,
            (Connection::Tunnel(a), Connection::Tunnel(b)) => a.join(b).await,

            (Connection::Tunnel(a), Connection::Direct(b)) => a.run(b).await,
            (Connection::Direct(a), Connection::Tunnel(b)) => b.run(a).await,
        };

        if let Err(e) = result {
            error!(target: log_target, "Route failed: {}", e);
        }
    }
}
