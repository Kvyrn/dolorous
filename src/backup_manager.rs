use crate::compressor::{
    Compressor, CopyCompressor, TarCompressor, TarGzCompressor, ZipCompressor,
};
use crate::configs::{BackupFileType, BackupsConfig};
use chrono::Local;
use color_eyre::eyre::{bail, WrapErr};
use color_eyre::Result;
use globwalk::GlobWalkerBuilder;
use new_string_template::template::Template;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;
use tracing::{debug, info, info_span};

pub fn run_backup(config: &BackupsConfig) -> Result<PathBuf> {
    let _e = info_span!("run_backup").entered();
    let name = render_name(config.name.clone(), &config.time_format, &config.file_type)?;
    let file_path = config.output.as_path().join(&name);
    (match &config.file_type {
        BackupFileType::Zip => create_backup::<ZipCompressor>,
        BackupFileType::TarGz => create_backup::<TarGzCompressor<6>>,
        BackupFileType::TarGzFast => create_backup::<TarGzCompressor<1>>,
        BackupFileType::TarGzSmall => create_backup::<TarGzCompressor<9>>,
        BackupFileType::Tar => create_backup::<TarCompressor>,
        BackupFileType::Copy => create_backup::<CopyCompressor>,
    })(&config.location, file_path.clone(), &config.files)?;

    Ok(file_path)
}

fn create_backup<C: Compressor>(
    base_path: &Path,
    output_path: PathBuf,
    globs: &[String],
) -> Result<()> {
    let _e = info_span!(
        "create_backup",
        backup_type = C::NAME,
        output = ?output_path,
        ?base_path,
    )
    .entered();
    info!("Starting backup...");
    if output_path.exists() {
        bail!("Output path already exists");
    }
    let start = Instant::now();

    let mut compressor = C::new(output_path)?;
    for file in GlobWalkerBuilder::from_patterns(base_path, globs)
        .follow_links(true)
        .build()
        .wrap_err("Failed to create glob walker!")?
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
    {
        let size = compressor.add_file(
            file.path(),
            file.path()
                .strip_prefix(base_path)
                .wrap_err("File outside base path!")?,
        )?;
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
    let size = compressor.finish()?;
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

fn render_name(template: String, time_format: &str, file_type: &BackupFileType) -> Result<String> {
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
