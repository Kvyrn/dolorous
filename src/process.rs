use crate::configs::DolorousConfig;
use color_eyre::eyre::{eyre, WrapErr};
use color_eyre::Result;
use heapless::HistoryBuffer;
use parking_lot::Mutex;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::{mpsc, watch};
use tracing::{debug, error, info, info_span, instrument, warn, Instrument};

pub static OUTPUT_CACHE: Mutex<HistoryBuffer<u8, { 2usize.pow(10) * 8 }>> =
    Mutex::new(HistoryBuffer::new());
pub static OUTPUT_WATCH: Mutex<Option<watch::Receiver<String>>> = Mutex::new(None);
pub static STDIN: Mutex<Option<mpsc::UnboundedSender<String>>> = Mutex::new(None);

#[instrument(skip(config))]
pub async fn run(config: &DolorousConfig) -> Result<()> {
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
        .in_current_span()
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
        .in_current_span()
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
                if let Err(_err) = stdin.write_all(line.as_bytes()).await {
                    break;
                }
            }
            info!("Stdin closed");
            let _ = STDIN.lock().take();
        }
        .in_current_span()
        .instrument(info_span!("write_stdin", pid)),
    );

    info!("Child started: {}", pid);
    child.wait().await?;
    Ok(())
}
