use crate::configs::{DolorousConfig, StopProperties};
use crate::CONFIG;
use color_eyre::eyre::{eyre, WrapErr};
use color_eyre::Result;
use heapless::HistoryBuffer;
use nix::libc::pid_t;
use nix::sys::signal::Signal;
use nix::unistd::Pid;
use parking_lot::Mutex;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::select;
use tokio::sync::{mpsc, watch, OnceCell};
use tracing::{debug, error, info, info_span, instrument, warn, Instrument};

pub static OUTPUT_CACHE: Mutex<HistoryBuffer<u8, { 2usize.pow(10) * 8 }>> =
    Mutex::new(HistoryBuffer::new());
pub static OUTPUT_WATCH: Mutex<Option<watch::Receiver<String>>> = Mutex::new(None);
pub static STDIN: Mutex<Option<mpsc::UnboundedSender<String>>> = Mutex::new(None);
pub static CONTROL: OnceCell<mpsc::UnboundedSender<Controls>> = OnceCell::const_new();

#[instrument]
pub async fn deamon() -> Result<()> {
    let (control_sender, mut control_receiver) = mpsc::unbounded_channel::<Controls>();
    CONTROL
        .set(control_sender)
        .wrap_err("Already running!")
        .unwrap();

    tokio::spawn(
        async move {
            let config = CONFIG.get().unwrap();
            let mut child = start(config.process.restart_attempts, config.process.restart_delay, config).await;

            loop {
                if let Some(ch) = &mut child {
                    select! {
                        _ = tokio::time::sleep(Duration::from_secs(30)) => {
                            if let Ok(Some(status)) = ch.try_wait() {
                                if status.success() {
                                    info!("Child exited with code {}", status.code().unwrap_or_default());
                                } else {
                                    error!("Child exited with code {}", status.code().unwrap_or_default());
                                }
                                child = start(config.process.restart_attempts, config.process.restart_delay, config).await;
                            }
                        },
                        ctrl = control_receiver.recv() => {
                            if let Some(ctrl) = ctrl {
                                match ctrl {
                                    Controls::Start => {
                                        if child.is_none() {
                                            child = start(config.process.restart_attempts, config.process.restart_delay, config).await;
                                        }
                                    },
                                    Controls::Stop => {
                                        if let Some(ch) = &mut child {
                                            stop_wrapper(&config.process.stop_config, ch).await;
                                            drop(child.take());
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        .instrument(info_span!("deamon")),
    );

    Ok(())
}

async fn stop_wrapper(config: &StopProperties, child: &mut Child) {
    stop(config, child).await;
    // Clean up
    {
        *STDIN.lock() = None;
    }
    {
        *OUTPUT_WATCH.lock() = None;
    }
}

async fn stop(config: &StopProperties, child: &mut Child) {
    info!("Stopping child...");
    if child.try_wait().is_ok() {
        info!("Child already exited");
        return;
    }
    let sender = {
        let opt = STDIN.lock();
        opt.clone()
    };
    if let Some(sender) = sender {
        if sender.send(config.stop_command.clone()).is_err() {
            warn!("Failed to send stop command");
        } else if let Ok(status) = tokio::time::timeout(config.term_timeout, child.wait()).await {
            let exit_code = status
                .map(|s| s.code())
                .unwrap_or_default()
                .unwrap_or_default();
            info!("Child stopped with exit code {exit_code}");
        } else {
            warn!("Term timeout reached");
        }
    }

    if let Some(id) = child.id() {
        match nix::sys::signal::kill(Pid::from_raw(id as pid_t), Signal::SIGTERM) {
            Ok(_) => {
                if let Ok(status) = tokio::time::timeout(config.kill_timeout, child.wait()).await {
                    let exit_code = status
                        .map(|s| s.code())
                        .unwrap_or_default()
                        .unwrap_or_default();
                    info!("Child stopped with exit code {exit_code}");
                } else {
                    warn!("Kill timeout reached");
                }
            }
            Err(err) => {
                warn!(?err, "Failed to send sognal to child");
            }
        }
    } else {
        warn!("Unable to find child pid");
    }

    match child.kill().await {
        Ok(_) => {
            info!("Killed child :)");
        }
        Err(err) => {
            error!(?err, "Failed to kill child, giving up :(");
        }
    }
}

async fn start(attempts: i16, delay: Duration, config: &DolorousConfig) -> Option<Child> {
    let infinite_attempts = attempts < 0;
    let mut num_attempts = 1;
    loop {
        match run(config).await {
            Ok(c) => return Some(c),
            Err(err) => {
                error!(
                    ?err,
                    "Failed to start child: attempt {}/{}", num_attempts, attempts
                );
                tokio::time::sleep(delay).await;
            }
        }

        if !infinite_attempts {
            if num_attempts >= attempts {
                break;
            }
            num_attempts += 1;
        }
    }
    None
}

#[instrument(skip(config))]
pub async fn run(config: &DolorousConfig) -> Result<Child> {
    info!("Starting process");
    let command = shell_words::split(&config.process.command).wrap_err("Invalid command")?;
    let mut child = Command::new(&command[0])
        .args(&command[1..])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::piped())
        .current_dir(&config.process.working_directory)
        .spawn()
        .wrap_err("Failed to spawn child!")?;
    let pid = child.id().unwrap_or_default();

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| eyre!("Missing child stdout"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| eyre!("Missing child stderr"))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| eyre!("Missing child stdin!"))?;

    let (merge_sender, mut merge_receiver) = mpsc::unbounded_channel::<String>();
    let merge_sender_err = merge_sender.clone();
    // Stdout reader
    tokio::spawn(
        async move {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line).await {
                    Ok(n) if n < 1 => {
                        break;
                    }
                    Err(err) => {
                        error!(?err, "Reading stdout failed");
                        continue;
                    }
                    _ => {}
                }
                let mut cache = OUTPUT_CACHE.lock();
                debug!("Stdout: {line:?}");
                cache.extend_from_slice(line.as_bytes());
                let _ = merge_sender.send(line);
            }
            debug!("Stdout closed");
        }
        .instrument(info_span!("read_stdout", pid)),
    );

    // Stderr reader
    tokio::spawn(
        async move {
            let mut reader = BufReader::new(stderr);
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line).await {
                    Ok(n) if n < 1 => {
                        break;
                    }
                    Err(err) => {
                        error!(?err, "Reading stderr failed");
                        continue;
                    }
                    _ => {}
                }
                let mut cache = OUTPUT_CACHE.lock();
                debug!("Stderr: {line:?}");
                cache.extend_from_slice(line.as_bytes());
                let _ = merge_sender_err.send(line);
            }
            debug!("Stderr closed");
        }
        .instrument(info_span!("read_stderr", pid)),
    );

    let (watch_sender, watch_receiver) = watch::channel::<String>("".into());
    let _ = OUTPUT_WATCH.lock().insert(watch_receiver);

    // Output merger
    tokio::spawn(
        async move {
            while let Some(line) = merge_receiver.recv().await {
                if let Err(err) = watch_sender.send(line) {
                    warn!(?err, "Watch merge error");
                }
            }
        }
        .in_current_span()
        .instrument(info_span!("merge_output", pid)),
    );

    let (sender, mut receiver) = mpsc::unbounded_channel::<String>();
    let _ = STDIN.lock().insert(sender);

    tokio::spawn(
        async move {
            while let Some(line) = receiver.recv().await {
                if let Err(_err) = stdin.write_all(line.trim().as_bytes()).await {
                    break;
                }
                if let Err(_err) = stdin.write_all(b"\n").await {
                    break;
                }
            }
            info!("Stdin closed");
            let _ = STDIN.lock().take();
        }
        .instrument(info_span!("write_stdin", pid)),
    );

    info!("Child started: {}", pid);
    Ok(child)
}

#[derive(Debug)]
pub enum Controls {
    Start,
    Stop,
}
