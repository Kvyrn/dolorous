mod event_handlers;
mod run;
mod types;

use self::types::*;
use crate::configs::DolorousConfig;
use color_eyre::eyre::{eyre, WrapErr};
use color_eyre::Result;
use log_buffer::LogBuffer;
use nix::errno::Errno;
use nix::sys::wait::{waitpid, WaitStatus};
use parking_lot::Mutex;
use std::time::Duration;
use tokio::select;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::{mpsc, watch, OnceCell};
use tokio::time::Instant;
use tracing::{debug, error, instrument, warn};

pub static CONTROL: OnceCell<mpsc::UnboundedSender<Controls>> = OnceCell::const_new();
pub static OUTPUT_WATCH: Mutex<Option<watch::Receiver<String>>> = Mutex::new(None);
pub static STDIN: Mutex<Option<mpsc::UnboundedSender<String>>> = Mutex::new(None);
pub static OUTPUT_CACHE: OnceCell<Mutex<LogBuffer<Vec<u8>>>> = OnceCell::const_new();

#[instrument(skip(config))]
pub async fn deamon(config: &'static DolorousConfig) {
    let output_cache = Mutex::new(LogBuffer::new(vec![0; config.process.cache_size as usize]));
    OUTPUT_CACHE
        .set(output_cache)
        .wrap_err("Already running")
        .unwrap();

    let (control_sender, control_receiver) = mpsc::unbounded_channel();
    CONTROL
        .set(control_sender)
        .wrap_err("Already running")
        .unwrap();

    let (exit_sender, exit_receiver) = mpsc::unbounded_channel::<(i32, i32)>();
    start_exit_watcher(exit_sender);

    tokio::spawn(run_deamon(config, control_receiver, exit_receiver));
}

async fn run_deamon(
    config: &DolorousConfig,
    mut control_receiver: UnboundedReceiver<Controls>,
    mut exit_receiver: UnboundedReceiver<(i32, i32)>,
) {
    let mut wanted = WantedState::Running;
    let mut state = ProcessState::Stopped;

    loop {
        match (&wanted, &state) {
            (WantedState::Running, ProcessState::Stopped) => match run::start(config).await {
                Ok(pid) => {
                    let timeout_at = Instant::now() + config.process.watch_delay;
                    state = ProcessState::Watching {
                        pid,
                        timeout_at,
                        attempt: 1,
                    };
                }
                Err(err) => {
                    warn!(?err, "Failed to start server!");
                    state = ProcessState::WaitingRestart {
                        attempt: 2,
                        timeout_at: Instant::now() + config.process.restart_delay,
                    };
                }
            },
            (WantedState::Stopped, ProcessState::Running { pid }) => {
                match stop_server_command(config, *pid) {
                    Ok(s) => state = s,
                    Err(err) => {
                        error!(?err, "Failed to stop servr");
                    }
                }
            }
            _ => {}
        }

        let event = fetch_event(&mut control_receiver, &mut exit_receiver, &mut state).await;

        match event {
            Event::Start => {
                wanted = WantedState::Running;
            }
            Event::Stop => {
                wanted = WantedState::Stopped;
                if let ProcessState::Watching { pid, .. } = &state {
                    debug!("Stop request: skipping watching");
                    state = ProcessState::Running { pid: *pid };
                }
            }
            Event::ProcessExited { pid, exit_code } => {
                event_handlers::handle_exit_event(config, &mut state, pid, exit_code).await
            }
            Event::TimeoutReached => {
                event_handlers::handle_timeout_reached(config, &mut wanted, &mut state).await
            }
        }
    }
}

async fn fetch_event(
    control_receiver: &mut UnboundedReceiver<Controls>,
    exit_receiver: &mut UnboundedReceiver<(i32, i32)>,
    state: &mut ProcessState,
) -> Event {
    let timeout = match &state {
        ProcessState::Watching { timeout_at, .. } => Some(timeout_at),
        ProcessState::WaitingRestart { timeout_at, .. } => Some(timeout_at),
        ProcessState::Stopping(StoppingState::Command { timeout_at, .. }) => Some(timeout_at),
        ProcessState::Stopping(StoppingState::Terminate { timeout_at, .. }) => Some(timeout_at),
        _ => None,
    };

    match timeout {
        Some(t) => {
            select! {
                Some(control) = control_receiver.recv() => {
                    match control {
                        Controls::Start => Event::Start,
                        Controls::Stop => Event::Stop,
                    }
                },
                Some((pid, exit_code)) = exit_receiver.recv() => {
                    Event::ProcessExited { pid, exit_code }
                },
                _ = tokio::time::sleep_until(*t) => {
                    Event::TimeoutReached
                },
            }
        }
        None => {
            select! {
                Some(control) = control_receiver.recv() => {
                    match control {
                        Controls::Start => Event::Start,
                        Controls::Stop => Event::Stop,
                    }
                },
                Some((pid, exit_code)) = exit_receiver.recv() => {
                    Event::ProcessExited { pid, exit_code }
                },
            }
        }
    }
}

fn start_exit_watcher(channel: mpsc::UnboundedSender<(i32, i32)>) {
    std::thread::spawn(move || {
        loop {
            match waitpid(None, None) {
                Ok(WaitStatus::Exited(pid, exit_code)) => {
                    if let Err(err) = channel.send((pid.as_raw() as i32, exit_code)) {
                        error!(?err, "Exit send error");
                    }
                }
                Err(Errno::ECHILD) => {
                    // No child processes
                    std::thread::sleep(Duration::from_secs(1));
                }
                Err(err) => {
                    error!(?err, "Wait error");
                }
                // Ignore other wait statuses
                _ => {}
            }
        }
    });
}

fn stop_server_command(config: &DolorousConfig, pid: i32) -> Result<ProcessState> {
    let stdin_channel = STDIN
        .lock()
        .as_ref()
        .cloned()
        .ok_or_else(|| eyre!("Stdin unavailable"))?;
    stdin_channel.send(config.process.stop_config.stop_command.clone())?;
    let timeout_at = Instant::now() + config.process.stop_config.term_timeout;
    Ok(ProcessState::Stopping(StoppingState::Command {
        timeout_at,
        pid,
    }))
}

#[derive(Debug)]
pub enum Controls {
    Start,
    Stop,
}
