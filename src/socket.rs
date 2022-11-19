use crate::configs::DolorousConfig;
use color_eyre::eyre::WrapErr;
use color_eyre::Result;
use std::path::Path;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tracing::{debug, error, info, info_span, instrument, warn, Instrument};

#[instrument(skip(config))]
pub async fn setup(config: &DolorousConfig) -> Result<()> {
    let Some(socket_path) = &config.socket else {
        info!("No socket set");
        return Ok(());
    };
    run_socket(socket_path).await
}

#[instrument]
async fn run_socket(path: &Path) -> Result<()> {
    let listener = UnixListener::bind(path).wrap_err("Failed to bind socket")?;
    info!("Opened socket at {}", path.to_string_lossy());

    tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    let peer_cred = stream
                        .peer_cred()
                        .map(|c| format!("{c:?}"))
                        .unwrap_or_else(|_| "<unknown>".into());
                    tokio::spawn(
                        handle_client(stream).instrument(info_span!("handle_client", ?peer_cred)),
                    );
                }
                Err(err) => {
                    error!(?err, "Failed to accept connection");
                }
            }
        }
    });

    Ok(())
}

async fn handle_client(stream: UnixStream) -> Result<()> {
    debug!("Client connection opened");
    let (reader, mut writer) = stream.into_split();
    let opt = {
        let sender = crate::process::STDIN.lock();
        sender.clone()
    };
    let Some(channel) = opt else {
        writer.write_all(b"Uninitialized").await?;
        return Ok(());
    };
    let opt = {
        let watch = crate::process::OUTPUT_WATCH.lock();
        watch.clone()
    };
    let Some(mut watch) = opt else {
        writer.write_all(b"Uninitialized").await?;
        return Ok(());
    };
    let data = {
        let cache = crate::process::OUTPUT_CACHE.lock();
        cache.oldest_ordered().copied().collect::<Vec<_>>()
    };

    // Transport input to process
    tokio::spawn(
        async move {
            let mut reader = BufReader::new(reader);
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line).await {
                    Ok(n) if n < 1 => {
                        debug!("Client connection closed");
                        break;
                    }
                    Err(err) => {
                        warn!(?err, "Error receiving from client");
                        continue;
                    }
                    _ => {}
                }
                info!("To stdin: {:?}", line);
                if let Err(err) = channel.send(line) {
                    warn!(?err, "Send error");
                    break;
                }
            }
        }
        .in_current_span(),
    );

    // Transport process output to socket
    tokio::spawn(
        async move {
            if writer.write_all(&data).await.is_err() {
                return;
            }
            while watch.changed().await.is_ok() {
                let line = { watch.borrow().clone() };
                if writer.write_all(line.as_bytes()).await.is_err() {
                    break;
                }
            }
        }
        .in_current_span(),
    );
    Ok(())
}
