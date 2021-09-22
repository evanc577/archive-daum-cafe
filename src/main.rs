use crate::downloader::download;

use anyhow::Result;
use std::process;

mod config;
mod downloader;
mod cookies;
mod error;

#[tokio::main]
async fn main() {
    match run().await {
        Ok(_) => process::exit(0),
        Err(err) => {
            for cause in err.chain() {
                eprintln!("{}", cause);
            }
            process::exit(1);
        }
    }
}

async fn run() -> Result<()> {
    let config = config::read_config()?;
    download(&config).await?;
    Ok(())
}
