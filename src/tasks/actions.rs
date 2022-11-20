use crate::configs::ActionType;
use crate::process::Controls;
use crate::CONFIG;
use color_eyre::eyre::{bail, eyre};
use color_eyre::Result;

pub async fn execute_action(action: &ActionType) -> Result<()> {
    match action {
        ActionType::Backup { backup } => backup_action(backup).await,
        ActionType::Command { command } => command_action(command).await,
        ActionType::Start => start_action().await,
        ActionType::Stop => stop_action().await,
        ActionType::Restart => restart_action().await,
    }
}

async fn backup_action(backup: &str) -> Result<()> {
    let config = CONFIG.get().ok_or_else(|| eyre!("Missing config"))?;
    crate::backup_manager::run_backup(config, backup).await?;
    Ok(())
}

async fn command_action(command: &str) -> Result<()> {
    let sender = {
        let opt = crate::process::STDIN.lock();
        let Some(sender) = &*opt else {
            bail!("Uninitialized");
        };
        sender.clone()
    };
    sender.send(command.to_string())?;
    Ok(())
}

async fn start_action() -> Result<()> {
    let Some(control) = crate::process::CONTROL.get().cloned() else {
        bail!("Uninitialized");
    };
    control.send(Controls::Start)?;
    Ok(())
}

async fn stop_action() -> Result<()> {
    let Some(control) = crate::process::CONTROL.get().cloned() else {
        bail!("Uninitialized")
    };
    control.send(Controls::Stop)?;
    Ok(())
}

async fn restart_action() -> Result<()> {
    let Some(control) = crate::process::CONTROL.get().cloned() else {
        bail!("Uninitialized")
    };
    control.send(Controls::Stop)?;
    control.send(Controls::Start)?;
    Ok(())
}
