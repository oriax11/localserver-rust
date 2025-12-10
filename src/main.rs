pub mod cgi;
pub mod config;
pub mod error;
pub mod request;
pub mod router;
pub mod server;
pub mod utils;
pub(crate) mod response;

use server::Server;

fn main() {
    println!("Starting server...");

    let config = match config::load_config("config.yaml") {
        Ok(config) => {
            println!("Configuration loaded successfully!");
            config
        }
        Err(e) => {
            eprintln!("Failed to load configuration: {}", e);
            return;
        }
    };

    let mut server = match Server::new() {
        Ok(server) => server,
        Err(e) => {
            eprintln!("Failed to initialize server: {}", e);
            return;
        }
    };

    if let Err(e) = server.run(config) {
        eprintln!("Server error: {}", e);
    }
}
