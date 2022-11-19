mod actions;

use crate::configs::{DolorousConfig, TaskConfig};
use chrono::Local;
use color_eyre::Result;
use cron::Schedule;
use std::str::FromStr;
use tokio::time::Instant;
use tracing::{error, info_span, warn, Instrument, info};

pub async fn start(config: &DolorousConfig) -> Result<()> {
    for (name, cfg) in &config.tasks {
        tokio::spawn(task_scheduler(cfg.clone()).instrument(info_span!("task_scheduler", name)));
    }
    Ok(())
}

async fn task_scheduler(config: TaskConfig) {
    let Ok(schedule) = Schedule::from_str(&config.schedule) else {
        error!("Invalid task schedule!");
        return;
    };
    for datetime in schedule.upcoming(Local) {
        let Ok(time_until) = (datetime - Local::now()).to_std() else {
            warn!("Task deadline passed");
            continue;
        };
        tokio::time::sleep_until(Instant::now() + time_until).await;
        let actions = config.actions.clone();
        tokio::spawn(
            async move {
                info!("Running task...");
                for (index, action) in actions.iter().enumerate() {
                    if let Err(err) = actions::execute_action(action)
                        .instrument(info_span!("execute_action", index))
                        .await
                    {
                        error!(?err, "Error running task");
                    }
                }
            }
            .instrument(info_span!("run_task")),
        );
    }
}
