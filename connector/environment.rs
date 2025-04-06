use anyhow::{Context, Result};
use dotenvy::dotenv;
use log::LevelFilter;
use std::{
    env,
    net::{IpAddr, SocketAddr},
};
use tcp_tunnel::encryption::generate_secret_from_string;

#[derive(Clone, Debug)]
pub struct Environment {
    pub server_addr: SocketAddr,
    pub relay_addr: SocketAddr,
    pub secret: [u8; 32],
    pub connections: u16,
    pub log_level: LevelFilter,
}

impl Environment {
    pub fn new() -> Result<Self> {
        dotenv().context("failed to load dotenv")?;

        let relay_ip: IpAddr = env::var("RELAY_IP")
            .context("couldn't find RELAY_IP in dotenv")?
            .parse()
            .context("couldn't parse RELAY_IP")?;

        let server_ip: IpAddr = env::var("SERVER_IP")
            .context("couldn't find SERVER_IP in dotenv")?
            .parse()
            .context("couldn't parse SERVER_IP")?;

        let server_port: u16 = env::var("SERVER_PORT")
            .context("couldn't find SERVER_PORT in dotenv")?
            .parse()
            .context("couldn't parse SERVEr_PORT")?;

        let relay_port: u16 = env::var("SHARED_PORT")
            .context("couldn't find SHARED_PORT in dotenv")?
            .parse()
            .context("couldn't parse SHARED_PORT")?;

        let secret = env::var("SECRET").context("couldn't find SECRET in dotenv")?;

        let connections: u16 = env::var("CONNECTIONS")
            .context("couldn't find CONNECTIONS in dotenv")?
            .parse()
            .context("couldn't parse CONNECTIONS")?;

        let log_level: LevelFilter = match env::var("LOG_LEVEL")
            .context("couldn't find LOG_LEVEL in dotenv")?
            .parse::<u16>()
            .context("couldn't parse LOG_LEVEL")?
        {
            1 => LevelFilter::Error,
            2 => LevelFilter::Warn,
            3 => LevelFilter::Info,
            4 => LevelFilter::Debug,
            5 => LevelFilter::Trace,
            _ => LevelFilter::Off,
        };

        Ok(Self {
            server_addr: SocketAddr::new(server_ip, server_port),
            relay_addr: SocketAddr::new(relay_ip, relay_port),
            secret: generate_secret_from_string(secret),
            connections,
            log_level,
        })
    }
}
