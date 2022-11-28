use crate::configs::{DolorousConfig, RestartCondition};
use crate::process::types::{ProcessState, StoppingState, WantedState};
use crate::process::{run, OUTPUT_WATCH, STDIN};
use color_eyre::eyre::WrapErr;
use nix::sys::signal::{kill, Signal};
use nix::unistd::Pid;
use tokio::time::Instant;
use tracing::{debug, error, info, warn};

pub async fn handle_exit_event(
    config: &DolorousConfig,
    state: &mut ProcessState,
    pid: i32,
    exit_code: i32,
) {
    match &state {
        #[rustfmt::skip]
        ProcessState::Watching { pid: existing_pid, attempt, .. } if *existing_pid == pid => {
            warn!("Process exited during startup: attempt {}/{}, exit code {}", attempt, config.process.restart_attempts, exit_code);
            { *OUTPUT_WATCH.lock() = None; }
            { *STDIN.lock() = None; }
            let timeout_at = Instant::now() + config.process.restart_delay;
            *state = ProcessState::WaitingRestart { timeout_at, attempt: attempt + 1 };
        }
        ProcessState::Running { pid: exsisting_pid } if *exsisting_pid == pid => {
            if exit_code != 0 {
                warn!("Process exited with non-zero exit code {}", exit_code);
            } else {
                info!("Process exited with exit code 0");
            }

            let restart = matches!(
                (&config.process.restart, exit_code != 0),
                (RestartCondition::Always, _)
                    | (RestartCondition::IfCrashed, true)
                    | (RestartCondition::UnlessCrashed, false)
            );
            if restart {
                match run::start(config).await {
                    Ok(pid) => {
                        let timeout_at = Instant::now() + config.process.watch_delay;
                        *state = ProcessState::Watching {
                            pid,
                            timeout_at,
                            attempt: 1,
                        };
                    }
                    Err(err) => {
                        warn!(?err, "Failed to start server!");
                        *state = ProcessState::WaitingRestart {
                            attempt: 2,
                            timeout_at: Instant::now() + config.process.restart_delay,
                        };
                    }
                }
            }
        }
        ProcessState::Stopping(_) => {
            info!(exit_code, "Stopped server");
            *state = ProcessState::Stopped;
        }
        _ => {}
    }
}

pub async fn handle_timeout_reached(
    config: &DolorousConfig,
    wanted: &mut WantedState,
    state: &mut ProcessState,
) {
    match state {
        ProcessState::Watching { pid, .. } => {
            debug!(?pid, "Process started succesfully!");
            *state = ProcessState::Running { pid: *pid };
        }
        ProcessState::WaitingRestart { attempt, .. } => match run::start(config).await {
            Ok(pid) => {
                let timeout_at = Instant::now() + config.process.watch_delay;
                *state = ProcessState::Watching {
                    pid,
                    timeout_at,
                    attempt: *attempt,
                };
            }
            Err(err) => {
                if *attempt >= config.process.restart_attempts {
                    error!("Failed to start server");
                    *wanted = WantedState::Stopped;
                    *state = ProcessState::Stopped;
                } else {
                    warn!(?err, "Failed to start server, retriying");
                    *state = ProcessState::WaitingRestart {
                        attempt: *attempt + 1,
                        timeout_at: Instant::now() + config.process.restart_delay,
                    };
                }
            }
        },
        ProcessState::Stopping(StoppingState::Command { pid, .. }) => {
            warn!("Term timeout reached");
            match kill(Pid::from_raw(*pid), Signal::SIGTERM).wrap_err("Failed to send signal") {
                Ok(_) => {
                    *state = ProcessState::Stopping(StoppingState::Terminate {
                        pid: *pid,
                        timeout_at: Instant::now() + config.process.stop_config.kill_timeout,
                    })
                }
                Err(err) => {
                    error!(?err, "Failed to terminate");
                    *state = ProcessState::Stopping(StoppingState::Terminate {
                        pid: *pid,
                        timeout_at: Instant::now(),
                    })
                }
            }
        }
        ProcessState::Stopping(StoppingState::Terminate { pid, .. }) => {
            warn!("Kill timeout reached");
            if let Err(err) =
                kill(Pid::from_raw(*pid), Signal::SIGKILL).wrap_err("Failed to send signal")
            {
                error!(?err, "Failed to kill");
            }
            *state = ProcessState::Stopping(StoppingState::Kill);
        }
        _ => {}
    }
}
