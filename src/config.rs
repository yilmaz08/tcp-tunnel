use anyhow::Result;
use std::{collections::HashMap, fs};
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
        Ok(toml::from_str(&file_content)?)
    }
}
