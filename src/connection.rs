use crate::{
    config::{ConnectionType, Direction, Endpoint},
    encryption::generate_secret_from_string,
    error::TunnelError,
    tunnel::Tunnel,
};
use anyhow::Result;
use log::{debug, error, info};
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::Arc,
};
use tokio::{
    io::split,
    net::{TcpListener, TcpStream},
    sync::Mutex,
    time::{sleep, Duration, Instant},
};

const CONNREF_TIMEOUT: Duration = Duration::from_secs(5);
const SECRET_REJECTED_TIMEOUT: Duration = Duration::from_secs(30);
const NONCE_EARLY_EOF_TIMEOUT: Duration = Duration::from_secs(15);
const BAN_LENGTH: Duration = Duration::from_secs(60 * 5);

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
        ConnectionType::Tunnel => match &endpoint.secret {
            Some(secret) => Some(generate_secret_from_string(secret.to_owned())),
            None => return Err(TunnelError::NoSecret.into()),
        },
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
    ban_list: &Arc<Mutex<HashMap<IpAddr, Instant>>>,
) -> Result<Connection> {
    Ok(match &data {
        ConnectionData::Inbound {
            listener_lock,
            secret_option,
        } => {
            info!(target: log_target, "Listening for '{}'", endpoint_name);
            let listener = listener_lock.lock().await;
            let (stream, addr) = listener.accept().await?;
            drop(listener);

            let conn = if let Some(secret) = secret_option {
                if let Some(&time) = ban_list.lock().await.get(&addr.ip()) {
                    if time > Instant::now() {
                        return Err(TunnelError::ConnAttemptFromBannedIP.into());
                    }
                }
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

pub async fn start_connection(
    a: ConnectionData,
    b: ConnectionData,
    log_target: &str,
    ban_list: Arc<Mutex<HashMap<IpAddr, Instant>>>,
) {
    loop {
        let ts_a = match connect(&a, log_target, "Stream A", &ban_list).await {
            Ok(x) => x,
            Err(e) => {
                if let Some(io_error) = e.downcast_ref::<std::io::Error>() {
                    if io_error.kind() == std::io::ErrorKind::ConnectionRefused {
                        error!(target: log_target, "Connection refused! Sleeping for {:?}...", CONNREF_TIMEOUT);
                        sleep(CONNREF_TIMEOUT).await;
                        continue;
                    }
                } else if let Some(tunnel_error) = e.downcast_ref::<TunnelError>() {
                    match tunnel_error {
                        TunnelError::SecretRejected => {
                            error!(target: log_target, "{}: Sleeping for {:?}...", e, SECRET_REJECTED_TIMEOUT);
                            sleep(SECRET_REJECTED_TIMEOUT).await;
                            continue;
                        }
                        TunnelError::NonceEarlyEOF => {
                            error!(target: log_target, "{}: Sleeping for {:?}...", e, NONCE_EARLY_EOF_TIMEOUT);
                            sleep(NONCE_EARLY_EOF_TIMEOUT).await;
                            continue;
                        }
                        TunnelError::SecretMismatch(addr) | TunnelError::Timeout(addr) => {
                            // Ban ip
                            ban_list
                                .lock()
                                .await
                                .insert(*addr, Instant::now() + BAN_LENGTH);
                            info!(target: log_target, "{}: {} is banned for {:?}", e, addr, BAN_LENGTH);
                            continue;
                        }
                        _ => {}
                    }
                }
                error!(target: log_target, "Couldn't get stream A: {}", e);
                continue;
            }
        };

        let ts_b = match connect(&b, log_target, "Stream B", &ban_list).await {
            Ok(x) => x,
            Err(e) => {
                drop(ts_a);
                if let Some(io_error) = e.downcast_ref::<std::io::Error>() {
                    if io_error.kind() == std::io::ErrorKind::ConnectionRefused {
                        error!(target: log_target, "Connection refused! Sleeping for {:?}...", CONNREF_TIMEOUT);
                        sleep(CONNREF_TIMEOUT).await;
                        continue;
                    }
                } else if let Some(tunnel_error) = e.downcast_ref::<TunnelError>() {
                    match tunnel_error {
                        TunnelError::SecretRejected => {
                            error!(target: log_target, "{}: Sleeping for {:?}...", e, SECRET_REJECTED_TIMEOUT);
                            sleep(SECRET_REJECTED_TIMEOUT).await;
                            continue;
                        }
                        TunnelError::NonceEarlyEOF => {
                            error!(target: log_target, "{}: Sleeping for {:?}...", e, NONCE_EARLY_EOF_TIMEOUT);
                            sleep(NONCE_EARLY_EOF_TIMEOUT).await;
                            continue;
                        }
                        TunnelError::SecretMismatch(addr) | TunnelError::Timeout(addr) => {
                            // Ban ip
                            ban_list
                                .lock()
                                .await
                                .insert(*addr, Instant::now() + BAN_LENGTH);
                            info!(target: log_target, "{}: {} is banned for {:?}", e, addr, BAN_LENGTH);
                            continue;
                        }
                        _ => {}
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
