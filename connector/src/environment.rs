use std::env;
use dotenvy::dotenv;
use crate::encryption::generate_secret_from_string;

#[derive(Clone, Debug)]
pub struct Environment {
    pub server_host: String,
    pub relay_host: String,
    pub server_port: u16,
    pub relay_port: u16,
    pub secret: [u8; 32],
    pub connections: u16
}

impl Environment {
    pub fn new() -> Self {
        match dotenv() {
            Err(_) => panic!("dotenv couldn't be loaded!"),
            Ok(_) => println!("dotenv is loaded")
        }
        Self {
            secret: match env::var("SECRET") {
                Ok(val) => generate_secret_from_string(val),
                Err(_) => panic!("no SECRET found")
            },
            server_host: match env::var("SERVER_HOST") {
                Ok(val) => val,
                Err(_) => panic!("couldn't find HOST in dotenv")
            },
            relay_host: match env::var("RELAY_HOST") {
                Ok(val) => val,
                Err(_) => panic!("couldn't find HOST in dotenv")
            },
            relay_port: match env::var("RELAY_PORT") {
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
            }
        }
    }
}
