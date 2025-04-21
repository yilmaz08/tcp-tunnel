use anyhow::Result;
use config::{Endpoint, TunnelConfig};
use connection::{get_connection_data, ConnectionData};
use error::TunnelError;
use log::{warn, LevelFilter};
use std::collections::HashMap;
use tokio::task;

mod config;
mod connection;
mod encryption;
mod error;
mod tunnel;

async fn get_conndata_from_endpoint(
    name: &str,
    secret: [u8; 32],
    endpoint_conn_data: &mut HashMap<String, ConnectionData>,
    endpoints: &HashMap<String, Endpoint>,
) -> Result<ConnectionData> {
    if let Some(conn_data) = endpoint_conn_data.get(name) {
        return Ok(conn_data.clone());
    }
    let endpoint = match endpoints.get(name) {
        Some(x) => x,
        None => return Err(TunnelError::EndpointNotFound.into()),
    };
    let conn_data = get_connection_data(endpoint, secret).await?;
    endpoint_conn_data.insert(name.to_owned(), conn_data.clone());
    Ok(conn_data)
}

#[tokio::main]
async fn main() -> Result<()> {
    // Config
    let config_path = &std::env::var("TUNNEL_CONFIG").unwrap_or("Config.toml".to_owned());
    let config = TunnelConfig::load(config_path)?;

    // Logging
    let log_level: LevelFilter = match config.log_level {
        Some(0) => LevelFilter::Off,
        Some(1) => LevelFilter::Error,
        Some(2) => LevelFilter::Warn,
        Some(4) => LevelFilter::Debug,
        Some(5) => LevelFilter::Trace,
        _ => LevelFilter::Info, // Default
    };
    env_logger::builder().filter_level(log_level).init();

    // Encryption
    let secret: [u8; 32] = encryption::generate_secret_from_string(config.secret);

    // Connection
    let mut endpoint_conn_data: HashMap<String, ConnectionData> = HashMap::new();
    for (route_index, route) in config.routes.iter().enumerate() {
        // Check if it is a RouteToSelf
        if &route.endpoints[0] == &route.endpoints[1] {
            return Err(TunnelError::RouteToSelf.into());
        }
        // Get endpoint data
        let endpoint_a = get_conndata_from_endpoint(
            &route.endpoints[0],
            secret,
            &mut endpoint_conn_data,
            &config.endpoints,
        )
        .await?;
        let endpoint_b = get_conndata_from_endpoint(
            &route.endpoints[1],
            secret,
            &mut endpoint_conn_data,
            &config.endpoints,
        )
        .await?;
        // Generate worker tasks
        for conn_index in 0..route.size {
            task::spawn({
                let endpoint_a = endpoint_a.clone();
                let endpoint_b = endpoint_b.clone();
                async move {
                    connection::start_connection(
                        endpoint_a,
                        endpoint_b,
                        &format!("route #{} worker #{}", route_index, conn_index),
                    )
                    .await;
                }
            });
        }
    }

    // Warn about unused endpoints
    for (key, _) in config.endpoints {
        if !endpoint_conn_data.contains_key(&key) {
            warn!("Unused endpoint: {}", key);
        }
    }

    std::thread::park();
    Ok(())
}
