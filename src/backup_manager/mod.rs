use self::compressor::{Compressor, CopyCompressor, TarCompressor, TarGzCompressor, ZipCompressor};
use crate::configs::{BackupFileType, DolorousConfig};
use chrono::Local;
use color_eyre::eyre::{bail, eyre, WrapErr};
use color_eyre::Result;
use globwalk::GlobWalkerBuilder;
use new_string_template::template::Template;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;
use tracing::{debug, info, info_span, Instrument};

mod compressor;

#[tracing::instrument(skip(config))]
pub async fn run_backup(config: &DolorousConfig, backup: &str) -> Result<PathBuf> {
    let backup_config = config
        .backups
        .get(backup)
        .ok_or_else(|| eyre!("Undefined backup: {}", backup))?;
    let name = render_name(
        &backup_config.name,
        &backup_config.time_format,
        &backup_config.file_type,
    )?;
    let file_path = backup_config.output.as_path().join(&name);

    match &backup_config.file_type {
        BackupFileType::Zip => {
            create_backup_wrapped::<ZipCompressor>(
                &backup_config.location,
                file_path.clone(),
                &backup_config.files,
            )
            .await?
        }
        BackupFileType::TarGz => {
            create_backup_wrapped::<TarGzCompressor<6>>(
                &backup_config.location,
                file_path.clone(),
                &backup_config.files,
            )
            .await?
        }
        BackupFileType::TarGzFast => {
            create_backup_wrapped::<TarGzCompressor<1>>(
                &backup_config.location,
                file_path.clone(),
                &backup_config.files,
            )
            .await?
        }
        BackupFileType::TarGzSmall => {
            create_backup_wrapped::<TarGzCompressor<9>>(
                &backup_config.location,
                file_path.clone(),
                &backup_config.files,
            )
            .await?
        }
        BackupFileType::Tar => {
            create_backup_wrapped::<TarCompressor>(
                &backup_config.location,
                file_path.clone(),
                &backup_config.files,
            )
            .await?
        }
        BackupFileType::Copy => {
            create_backup_wrapped::<CopyCompressor>(
                &backup_config.location,
                file_path.clone(),
                &backup_config.files,
            )
            .await?
        }
    };

    Ok(file_path)
}

async fn create_backup_wrapped<C: Compressor>(
    base_path: &Path,
    output_path: PathBuf,
    globs: &[String],
) -> Result<()> {
    let outp = output_path.clone();
    create_backup::<C>(base_path, output_path, globs)
        .instrument(info_span!(
            "create_backup",
            backup_type = C::NAME,
            output = ?outp,
            ?base_path,
        ))
        .await
}

async fn create_backup<C: Compressor>(
    base_path: &Path,
    output_path: PathBuf,
    globs: &[String],
) -> Result<()> {
    info!("Starting backup...");
    if output_path.exists() {
        bail!("Output path already exists");
    }
    let start = Instant::now();

    let mut compressor = C::new(output_path).await?;
    for file in GlobWalkerBuilder::from_patterns(base_path, globs)
        .follow_links(true)
        .build()
        .wrap_err("Failed to create glob walker!")?
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
    {
        let size = compressor
            .add_file(
                file.path(),
                file.path()
                    .strip_prefix(base_path)
                    .wrap_err("File outside base path!")?,
            )
            .await?;
        let human_size = if size.is_nan() {
            "unknown".into()
        } else {
            human_bytes::human_bytes(size)
        };
        debug!(
            "Compressed file {:?} (original size: {})",
            file.path(),
            human_size
        );
    }
    let size = compressor.finish().await?;
    let elapsed = humantime::format_duration(start.elapsed());
    let human_size = if size.is_nan() {
        "unknown".into()
    } else {
        human_bytes::human_bytes(size)
    };
    info!(
        "Backup complete! (size: {}, elapsed: {})",
        human_size, elapsed
    );
    Ok(())
}

fn render_name(template: &str, time_format: &str, file_type: &BackupFileType) -> Result<String> {
    let template = Template::new(template);
    let data = {
        let mut map = HashMap::new();
        map.insert("date", format!("{}", Local::now().format(time_format)));
        map.insert("extension", find_extension(file_type).to_string());
        map
    };
    template.render(&data).wrap_err("Failed to render name!")
}

fn find_extension(typ: &BackupFileType) -> &str {
    match typ {
        BackupFileType::Zip => "zip",
        BackupFileType::TarGz | BackupFileType::TarGzSmall | BackupFileType::TarGzFast => "tar.gz",
        BackupFileType::Tar => "tar",
        BackupFileType::Copy => "d",
    }
}
