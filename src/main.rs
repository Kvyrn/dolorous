mod backup_manager;
mod compressor;
mod configs;

use crate::configs::DolorousConfig;
use clap::Parser;
use color_eyre::eyre::WrapErr;
use color_eyre::Result;
use figment::providers::{Format, Serialized, Yaml};
use figment::Figment;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
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

fn main() -> Result<()> {
    color_eyre::install()?;
    let args = Args::parse();
    let config: DolorousConfig =
        Figment::from(Serialized::from(DolorousConfig::default(), "default"))
            .merge(Yaml::file(args.config))
            .extract()
            .wrap_err("Failed to read config!")?;

    if std::env::var("DOLOROUS_LOG").is_err() {
        std::env::set_var("DOLOROUS_LOG", &config.log);
    }
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_env("DOLOROUS_LOG"))
        .init();

    backup_manager::run_backup(config.backups.get("default").unwrap())?;

    Ok(())
}
