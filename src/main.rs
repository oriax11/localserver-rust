pub mod cgi;
pub mod config;
pub mod error;
pub mod request;
pub mod router;
pub mod server;
pub mod utils;
pub(crate) mod response;
pub mod handler;
pub mod auth;

use server::Server;

fn main() {
    println!("Starting server...");

    let config = match config::load_config("config.yaml") {
        Ok(cfg) => {
            println!("Configuration loaded successfully!");
            cfg
        }
        Err(e) => {
            eprintln!("Failed to load configuration: {}", e);
            return;
        }
    };

    let mut server = match Server::new() {
        Ok(srv) => srv,
        Err(e) => {
            eprintln!("Failed to initialize server: {}", e);
            return;
        }
    };

    if let Err(e) = server.run(config) {
        eprintln!("Server error: {}", e);
    }
}
