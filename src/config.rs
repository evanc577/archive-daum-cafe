use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;

static CONFIG_FILE: &str = "config.toml";

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub cookies_file: String,
    #[serde(default = "default_num_processes")]
    pub max_connections: usize,
    pub cafe: HashMap<String, CafeConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct CafeConfig {
    pub download_path: Option<String>,
    pub boards: Vec<String>,
}

fn default_num_processes() -> usize {
    20
}

pub fn read_config() -> Result<Config> {
    let conf_contents =
        fs::read_to_string(CONFIG_FILE).context(format!("Error reading {}", CONFIG_FILE))?;
    let conf: Config =
        toml::from_str(&conf_contents).context(format!("Error parsing {}", CONFIG_FILE))?;
    Ok(conf)
}
