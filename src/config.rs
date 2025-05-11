use crate::{
    connection::{self, ConnectionData},
    error::ConfigError,
};
use anyhow::Result;
use futures::future::try_join_all;
use log::{warn, LevelFilter};
use std::{
    collections::{HashMap, HashSet},
    fs,
};
use toml;

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConnectionType {
    Tunnel,
    Direct,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Inbound,
    Outbound,
}

#[derive(Debug, serde::Deserialize)]
pub struct VeloxidConfig {
    pub routes: Vec<Route>,
    pub endpoints: HashMap<String, Endpoint>,
    pub log_level: Option<u8>,
}

#[derive(Debug, serde::Deserialize)]
pub struct Endpoint {
    pub host: Option<String>,
    pub port: u16,
    #[serde(rename = "type")]
    pub kind: ConnectionType,
    pub direction: Direction,
    pub secret: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
pub struct Route {
    pub endpoints: [String; 2],
    pub size: usize,
}

impl VeloxidConfig {
    pub fn load(file_path: &str) -> Result<Self> {
        let file_content = fs::read_to_string(file_path)?;
        let config: VeloxidConfig = toml::from_str(&file_content)?;
        VeloxidConfig::start_logging(config.log_level);
        Ok(config)
    }

    pub async fn get_endpoint_map(&self) -> Result<HashMap<String, ConnectionData>> {
        // Get unique endpoint names
        let mut names: HashSet<&str> = HashSet::new();
        for route in &self.routes {
            if route.endpoints[0] == route.endpoints[1] {
                return Err(ConfigError::RouteToSelf.into());
            }
            names.extend(route.endpoints.iter().map(String::as_str));
        }

        // Get all connection data in parallel
        let futures = names.iter().map(|&name| async move {
            let endpoint = self
                .endpoints
                .get(name)
                .ok_or(ConfigError::EndpointNotFound)?;
            let conn_data = connection::get_connection_data(endpoint).await?;
            Ok::<_, anyhow::Error>((name.to_owned(), conn_data))
        });

        // Return results
        let results = try_join_all(futures).await?.into_iter().collect();
        self.warn_unused_endpoints(&results);
        Ok(results)
    }

    fn warn_unused_endpoints(&self, endpoint_map: &HashMap<String, ConnectionData>) {
        for (key, _) in &self.endpoints {
            if !endpoint_map.contains_key(key) {
                warn!("Unused endpoint: {}", key);
            }
        }
    }

    fn start_logging(log_level: Option<u8>) {
        let level_filter: LevelFilter = match log_level {
            Some(0) => LevelFilter::Off,
            Some(1) => LevelFilter::Error,
            Some(2) => LevelFilter::Warn,
            Some(4) => LevelFilter::Debug,
            Some(5) => LevelFilter::Trace,
            _ => LevelFilter::Info, // Default
        };
        env_logger::builder().filter_level(level_filter).init();
    }
}
