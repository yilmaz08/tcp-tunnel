use anyhow::Result;
use config::TunnelConfig;
use connection::ConnectionData;
use log::LevelFilter;
use std::collections::HashMap;
use tokio::task;

mod config;
mod connection;
mod encryption;
mod error;
mod tunnel;

#[tokio::main]
async fn main() -> Result<()> {
    let config_path = &std::env::var("TUNNEL_CONFIG").unwrap_or("Config.toml".to_owned());
    let config = TunnelConfig::load(config_path)?;

    let log_level: LevelFilter = match config.log_level {
        Some(0) => LevelFilter::Off,
        Some(1) => LevelFilter::Error,
        Some(2) => LevelFilter::Warn,
        Some(4) => LevelFilter::Debug,
        Some(5) => LevelFilter::Trace,
        _ => LevelFilter::Info, // Default
    };
    env_logger::builder().filter_level(log_level).init();

    let secret: [u8; 32] = encryption::generate_secret_from_string(config.secret);

    let mut endpoint_data: HashMap<String, ConnectionData> = HashMap::new();
    for (key, value) in config.endpoints {
        endpoint_data.insert(key, connection::get_connection_data(&value, secret).await?);
    }

    for (route_index, route) in config.routes.iter().enumerate() {
        let endpoint_a = endpoint_data.get(&route.endpoints[0]).unwrap();
        let endpoint_b = endpoint_data.get(&route.endpoints[1]).unwrap();
        for conn_index in 0..route.size {
            task::spawn({
                let endpoint_a = endpoint_a.clone();
                let endpoint_b = endpoint_b.clone();
                async move {
                    connection::start_connection(
                        endpoint_a,
                        endpoint_b,
                        &format!("route #{} conn #{}", route_index, conn_index),
                    )
                    .await;
                }
            });
        }
    }

    std::thread::park();
    Ok(())
}
