use color_eyre::eyre::{bail, eyre};
use crate::configs::ActionType;
use color_eyre::Result;
use crate::CONFIG;

pub async fn execute_action(action: &ActionType) -> Result<()> {
    match action {
        ActionType::Backup { backup } => backup_action(backup).await,
        ActionType::Command { command } => command_action(command).await,
        ActionType::Start => unimplemented!(),
        ActionType::Stop { properties } => unimplemented!(),
        ActionType::Restart { properties } => unimplemented!(),
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
