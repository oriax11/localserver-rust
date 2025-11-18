use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub servers: Vec<ServerConfig>,
}

#[derive(Debug, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub ports: Vec<u16>,
    pub error_pages: HashMap<u16, String>,
    pub client_max_body_size: usize,
    pub routes: Vec<Route>,
}

#[derive(Debug, Deserialize)]
pub struct Route {
    pub path: String,
    pub methods: Vec<String>,
    pub root: Option<String>,
    pub default_file: Option<String>,
    pub redirect: Option<String>,
    pub cgi: Option<String>,
    pub list_directory: Option<bool>,
}

pub fn load_config(path: &str) -> Result<Config, Box<dyn std::error::Error>> {
    let file_content = std::fs::read_to_string(path)?;
    let config: Config = serde_yaml::from_str(&file_content)?;
    Ok(config)
}

