mod backup_manager;
mod configs;
mod process;
mod socket;
mod tasks;

use crate::configs::DolorousConfig;
use crate::process::Controls;
use clap::Parser;
use color_eyre::eyre::WrapErr;
use color_eyre::Result;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use nix::sys::wait::wait;
use tokio::select;
use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::OnceCell;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

static CONFIG: OnceCell<DolorousConfig> = OnceCell::const_new();
static EXITING: AtomicBool = AtomicBool::new(false);

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
    CONFIG.set(config).unwrap();
    let config = CONFIG.get().unwrap();

    //backup_manager::run_backup(&config, "default").await?;
    socket::setup(config).await?;
    tasks::start(config).await?;
    process::deamon(config).await;

    let mut term_sig = signal(SignalKind::terminate())?;
    let mut int_sig = signal(SignalKind::interrupt())?;

    select! {
        _ = term_sig.recv() => {},
        _ = int_sig.recv() => {},
    }
    info!("Stopping...");
    EXITING.store(true, Ordering::Relaxed);
    if let Some(control) = process::CONTROL.get() {
        let _ = control.send(Controls::Stop);
    }
    // Wait for child exit
    let _ = wait();
    if let Some(path) = &config.socket {
        info!("Removing socket");
        if let Err(err) = tokio::fs::remove_file(path).await {
            error!(?err, "Failed to delete socket");
        }
    }
    info!("Stopped!");
    std::process::exit(0);
}
