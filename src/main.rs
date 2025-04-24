use anyhow::Result;
use config::{Endpoint, Route, VeloxidConfig};
use connection::ConnectionData;
use dashmap::DashMap;
use error::ConfigError;
use futures::future::try_join_all;
use log::{info, warn, LevelFilter};
use std::{
    collections::{HashMap, HashSet},
    net::IpAddr,
};
use tokio::{task, time::Instant};

mod config;
mod connection;
mod encryption;
mod error;
mod tunnel;

async fn build_conn_map(
    routes: &[Route],
    config_endpoints: &HashMap<String, Endpoint>,
) -> Result<HashMap<String, ConnectionData>> {
    // Get unique endpoint names
    let mut names: HashSet<&str> = HashSet::new();
    for route in routes {
        names.extend(route.endpoints.iter().map(String::as_str));
    }

    // Get all connection data in parallel
    let futures = names.iter().map(|&name| async move {
        let endpoint = config_endpoints
            .get(name)
            .ok_or(ConfigError::EndpointNotFound)?;
        let conn_data = connection::get_connection_data(endpoint).await?;
        Ok::<_, anyhow::Error>((name.to_owned(), conn_data))
    });

    // Collect results
    let results = try_join_all(futures).await?;
    Ok(results.into_iter().collect())
}

#[tokio::main]
async fn main() -> Result<()> {
    // Config
    let config_path = &std::env::var("VELOXID_CONFIG").unwrap_or("veloxid.toml".to_owned());
    let config = VeloxidConfig::load(config_path)?;

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
    let ban_list: DashMap<IpAddr, Instant> = DashMap::new();

    // Connection
    let endpoint_conn_data = build_conn_map(&config.routes, &config.endpoints).await?;
    for (route_idx, route) in config.routes.iter().enumerate() {
        // Check if it is a RouteToSelf
        let [a, b] = &route.endpoints;
        if a == b {
            return Err(ConfigError::RouteToSelf.into());
        }

        // Get endpoint data
        let endpoint_a = &endpoint_conn_data[a];
        let endpoint_b = &endpoint_conn_data[b];

        // Generate worker tasks
        for worker_idx in 0..route.size {
            task::spawn({
                let endpoint_a = endpoint_a.clone();
                let endpoint_b = endpoint_b.clone();
                let ban_list = ban_list.clone();
                async move {
                    connection::route(
                        endpoint_a,
                        endpoint_b,
                        ban_list,
                        &format!("route #{} worker #{}", route_idx, worker_idx),
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

    // Wait for Ctrl+C
    tokio::signal::ctrl_c().await?;
    info!("Shutting down...");
    Ok(())
}
