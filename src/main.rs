mod backup_manager;
mod compressor;
mod configs;
mod process;
mod socket;

use std::fs::File;
use crate::configs::DolorousConfig;
use clap::Parser;
use color_eyre::eyre::WrapErr;
use color_eyre::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug, Deserialize, Serialize)]
struct Args {
    /// Configuration file
    #[arg(
        short,
        long,
        env = "DOLOROUS_CONFIG",
        value_name = "FILE",
        default_value = "/etc/dolorous/config.yml"
    )]
    config: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    let args = Args::parse();
    let config: DolorousConfig =
        serde_yaml::from_reader(File::open(&args.config).wrap_err("Failed to read config")?)
            .wrap_err("Failed to read config!")?;

    if std::env::var("DOLOROUS_LOG").is_err() {
        std::env::set_var("DOLOROUS_LOG", &config.log_filter);
    }
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_env("DOLOROUS_LOG"))
        .init();

    info!("{:#?}", config);
    //backup_manager::run_backup(&config, "default").await?;
    socket::setup(&config).await?;
    process::run(&config).await?;

    Ok(())
}
