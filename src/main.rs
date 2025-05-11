use anyhow::Result;
use config::VeloxidConfig;
use connection::ConnectionData;
use dashmap::DashMap;
use futures::future::join_all;
use log::{info, debug};
use std::{
    collections::HashMap,
    net::IpAddr,
    sync::{atomic::AtomicBool, Arc},
};
use tokio::{
    signal::{unix::signal, unix::SignalKind},
    task::{self, JoinHandle},
    time::Instant,
};

mod config;
mod connection;
mod encryption;
mod error;
mod tunnel;

async fn start_workers(
    endpoint_map: HashMap<String, ConnectionData>,
    routes: Vec<config::Route>,
    shutdown_bool: Arc<AtomicBool>,
) -> Result<Vec<JoinHandle<()>>> {
    let mut handles: Vec<JoinHandle<()>> = Vec::new();
    let ban_list: DashMap<IpAddr, Instant> = DashMap::new();

    for (route_idx, route) in routes.iter().enumerate() {
        // Get endpoint data
        let [a, b] = &route.endpoints;
        let endpoint_a = &endpoint_map[a];
        let endpoint_b = &endpoint_map[b];

        // Generate worker tasks
        for worker_idx in 0..route.size {
            let handle = task::spawn({
                let endpoint_a = endpoint_a.clone();
                let endpoint_b = endpoint_b.clone();
                let ban_list = ban_list.clone();
                let shutdown_bool = shutdown_bool.clone();
                async move {
                    connection::route(
                        endpoint_a,
                        endpoint_b,
                        ban_list,
                        shutdown_bool,
                        &format!("route #{} worker #{}", route_idx, worker_idx),
                    )
                    .await;
                }
            });
            handles.push(handle);
        }
    }
    Ok(handles)
}

#[tokio::main]
async fn main() -> Result<()> {
    // Signals
    let mut shutdown_signal = signal(SignalKind::terminate())?; // TERM
    let mut interrupt_signal = signal(SignalKind::interrupt())?; // INT

    // Config
    let config_path = &std::env::var("VELOXID_CONFIG").unwrap_or("veloxid.toml".to_owned());
    let config = VeloxidConfig::load(config_path)?;
    let endpoint_map = config.get_endpoint_map().await?;

    let shutdown_bool = Arc::new(AtomicBool::new(true));

    // Connection
    let handles = start_workers(endpoint_map, config.routes, shutdown_bool.clone()).await?;

    // Exit
    tokio::select! {
        _ = interrupt_signal.recv() => {},
        _ = shutdown_signal.recv() => {
            shutdown_bool.store(false, std::sync::atomic::Ordering::Relaxed);
            debug!("Waiting for all workers to exit...");
            join_all(handles).await;
        }
    }

    info!("Shutting down...");
    Ok(())
}
