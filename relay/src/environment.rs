use std::env;
use dotenvy::dotenv;
use log::LevelFilter;
use crate::encryption::generate_secret_from_string;

#[derive(Clone, Debug)]
pub struct Environment {
    pub host: String,
    pub server_port: u16,
    pub client_port: u16,
    pub secret: [u8; 32],
    pub connections: u16,
    pub log_level: LevelFilter
}

impl Environment {
    pub fn new() -> Option<Self> {
        match dotenv() {
            Err(_) => {
                panic!("dotenv couldn't be loaded")
            },
            Ok(_) => {}
        }
        Some(Self {
            secret: match env::var("SECRET") {
                Ok(val) => generate_secret_from_string(val),
                Err(_) => panic!("no SECRET found")
            },
            host: match env::var("HOST") {
                Ok(val) => val,
                Err(_) => panic!("couldn't find HOST in dotenv")
            },
            client_port: match env::var("CLIENT_PORT") {
                Ok(val) => val.parse::<u16>().unwrap(),
                Err(_) => panic!("couldn't find CLIENT_PORT in dotenv")
            },
            server_port: match env::var("SERVER_PORT") {
                Ok(val) => val.parse::<u16>().unwrap(),
                Err(_) => panic!("couldn't find SERVER_PORT in dotenv")
            },
            connections: match env::var("CONNECTIONS") {
                Ok(val) => val.parse::<u16>().unwrap(),
                Err(_) => panic!("couldn't find CONNECTIONS in dotenv")
            },
            log_level: match env::var("LOG_LEVEL") {
                Ok(val) => {
                    match val.parse::<u16>() {
                        Ok(0) => LevelFilter::Off,
                        Ok(1) => LevelFilter::Error,
                        Ok(2) => LevelFilter::Warn,
                        Ok(3) => LevelFilter::Info,
                        Ok(4) => LevelFilter::Debug,
                        Ok(5) => LevelFilter::Trace,
                        _ => panic!("couldn't parse LOG_LEVEL")
                    }
                },
                Err(_) => panic!("couldn't find LOG_LEVEL in dotenv")
            }
        })
    }
}
