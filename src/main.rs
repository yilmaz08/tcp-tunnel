use anyhow::Result;
use config::{Endpoint, TunnelConfig};
use connection::ConnectionData;
use error::TunnelError;
use log::{warn, LevelFilter};
use std::{collections::HashMap, net::IpAddr, sync::Arc};
use tokio::{sync::Mutex, task, time::Instant};

mod config;
mod connection;
mod encryption;
mod error;
mod tunnel;

async fn get_conndata_from_endpoint(
    name: &str,
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
    let conn_data = connection::get_connection_data(endpoint).await?;
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

    // Ban list
    let ban_list = Arc::new(Mutex::new(HashMap::<IpAddr, Instant>::new()));

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
            &mut endpoint_conn_data,
            &config.endpoints,
        )
        .await?;
        let endpoint_b = get_conndata_from_endpoint(
            &route.endpoints[1],
            &mut endpoint_conn_data,
            &config.endpoints,
        )
        .await?;
        // Generate worker tasks
        for conn_index in 0..route.size {
            task::spawn({
                let endpoint_a = endpoint_a.clone();
                let endpoint_b = endpoint_b.clone();
                let ban_list = ban_list.clone();
                async move {
                    connection::start_connection(
                        endpoint_a,
                        endpoint_b,
                        &format!("route #{} worker #{}", route_index, conn_index),
                        ban_list,
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
