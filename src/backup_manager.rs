use crate::configs::{BackupFileType, BackupsConfig};
use chrono::Local;
use color_eyre::eyre::{bail, eyre, WrapErr};
use color_eyre::Result;
use flate2::write::GzEncoder;
use flate2::Compression;
use globwalk::GlobWalkerBuilder;
use new_string_template::template::Template;
use std::collections::HashMap;
use std::fs::File;
use std::path::Path;
use std::time::Instant;
use tracing::{debug, info, info_span};
use zip::write::FileOptions;
use zip::ZipWriter;

pub struct BackupManager {
    types: HashMap<String, BackupsConfig>,
}

impl BackupManager {
    pub fn new(types: HashMap<String, BackupsConfig>) -> Self {
        Self { types }
    }

    #[tracing::instrument(skip(self))]
    pub fn run_backup(&self, name: &str) -> Result<()> {
        let Some(config) = self.types.get(name) else {
            bail!("Unknown config!");
        };
        let name = render_name(config.name.clone(), &config.time_format, &config.file_type)?;
        let file_path = config.output.as_path().join(&name);
        (match &config.file_type {
            BackupFileType::Zip => create_zip_backup,
            BackupFileType::TarGz => create_targz_backup,
        })(config.location.as_path(), &file_path, &config.files)?;

        Ok(())
    }
}

fn create_targz_backup(base_path: &Path, output_path: &Path, globs: &[String]) -> Result<()> {
    let _e = info_span!(
        "create_backup",
        file_type = "targz",
        output = output_path.to_string_lossy().as_ref()
    )
    .entered();
    info!("Starting backup...");
    if output_path.exists() {
        bail!("Path already exists");
    }
    let start = Instant::now();

    let compressor = GzEncoder::new(
        File::create(output_path).wrap_err("Failed to open output file")?,
        Compression::default(),
    );
    let mut writer = tar::Builder::new(compressor);
    for file in GlobWalkerBuilder::from_patterns(base_path, globs)
        .follow_links(true)
        .build()
        .wrap_err("Failed to create glob walker!")?
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
    {
        writer
            .append_file(
                file.path(),
                &mut File::open(file.path()).wrap_err("Failed to open file")?,
            )
            .wrap_err("Failed to compress file")?;
    }

    writer.finish().wrap_err("Failed to compress files")?;
    drop(writer);
    let elapsed = humantime::format_duration(start.elapsed());
    let output_size = std::fs::metadata(output_path)
        .map(|r| human_bytes::human_bytes(r.len() as f64))
        .unwrap_or_else(|_| "unknown".into());
    info!(
        "Backup complete (size: {}, elapsed: {})",
        output_size, elapsed
    );
    Ok(())
}

fn create_zip_backup(base_path: &Path, output_path: &Path, globs: &[String]) -> Result<()> {
    let _e = info_span!(
        "create_backup",
        file_type = "zip",
        output = output_path.to_string_lossy().as_ref()
    )
    .entered();
    info!("Starting backup...");
    if output_path.exists() {
        bail!("Path already exists");
    }
    let start = Instant::now();

    let mut writer =
        ZipWriter::new(File::create(output_path).wrap_err("Failed to open output file")?);
    for file in GlobWalkerBuilder::from_patterns(base_path, globs)
        .follow_links(true)
        .build()
        .wrap_err("Failed to create glob walker!")?
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
    {
        writer.start_file(
            file.path()
                .to_str()
                .ok_or_else(|| eyre!("Invalid file name!"))?,
            FileOptions::default(),
        )?;
        let mut input_file = File::open(file.path()).wrap_err("Failed to open file!")?;
        let compressed =
            std::io::copy(&mut input_file, &mut writer).wrap_err("Failed to compress file")?;
        debug!(
            "File {}: compressed {}",
            file.path().to_string_lossy(),
            human_bytes::human_bytes(compressed as f64)
        );
    }

    let output = writer.finish().wrap_err("Failed to compress files")?;
    let elapsed = humantime::format_duration(start.elapsed());
    let output_size = output
        .metadata()
        .map(|r| human_bytes::human_bytes(r.len() as f64))
        .unwrap_or_else(|_| "unknown".into());
    info!(
        "Backup complete (size: {}, elapsed: {})",
        output_size, elapsed
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
        BackupFileType::TarGz => "tar.gz",
    }
}
