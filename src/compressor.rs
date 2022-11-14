use color_eyre::eyre::{bail, eyre, ContextCompat, WrapErr};
use color_eyre::Result;
use flate2::write::GzEncoder;
use flate2::Compression;
use std::fs::File;
use std::path::{Path, PathBuf};
use zip::write::FileOptions;
use zip::ZipWriter;

pub trait Compressor {
    const NAME: &'static str;
    fn new(path: PathBuf) -> Result<Box<Self>>;
    /// Returns: size of original size
    fn add_file(&mut self, path: &Path, relative_path: &Path) -> Result<f64>;
    /// Returns: size of compressed file
    fn finish(self) -> Result<f64>;
}

pub struct ZipCompressor {
    writer: ZipWriter<File>,
}

impl Compressor for ZipCompressor {
    const NAME: &'static str = "zip";

    #[tracing::instrument]
    fn new(path: PathBuf) -> Result<Box<Self>> {
        let writer = ZipWriter::new(File::create(&path).wrap_err("Failed to create output file!")?);
        Ok(Box::new(Self { writer }))
    }

    #[tracing::instrument(skip(self))]
    fn add_file(&mut self, path: &Path, relative_path: &Path) -> Result<f64> {
        self.writer.start_file(
            relative_path
                .to_str()
                .ok_or_else(|| eyre!("Invalid file name"))?,
            FileOptions::default(),
        )?;
        let mut input_file = File::open(path).wrap_err("Failed to open file")?;
        let compressed = std::io::copy(&mut input_file, &mut self.writer)
            .wrap_err("Failed to compress file!")?;
        Ok(compressed as f64)
    }

    #[tracing::instrument(skip(self))]
    fn finish(mut self) -> Result<f64> {
        let output = self.writer.finish().wrap_err("Failed to compress files")?;
        let output_size = output
            .metadata()
            .map(|r| r.len() as f64)
            .unwrap_or(f64::NAN);
        Ok(output_size)
    }
}

pub struct TarGzCompressor<const LEVEL: u32> {
    writer: tar::Builder<GzEncoder<File>>,
    path: PathBuf,
}

impl<const LEVEL: u32> Compressor for TarGzCompressor<LEVEL> {
    const NAME: &'static str = "targz";

    #[tracing::instrument]
    fn new(path: PathBuf) -> Result<Box<Self>> {
        let compressor = GzEncoder::new(
            File::create(&path).wrap_err("Failed to open file")?,
            Compression::new(LEVEL),
        );
        let writer = tar::Builder::new(compressor);
        Ok(Box::new(Self { writer, path }))
    }

    #[tracing::instrument(skip(self))]
    fn add_file(&mut self, path: &Path, relative_path: &Path) -> Result<f64> {
        let mut file = File::open(path).wrap_err("Failed to open file")?;
        self.writer
            .append_file(relative_path, &mut file)
            .wrap_err("Failed to compress file")?;
        let size = file.metadata().map(|m| m.len() as f64).unwrap_or(f64::NAN);
        Ok(size)
    }

    #[tracing::instrument(skip(self))]
    fn finish(mut self) -> Result<f64> {
        self.writer.finish().wrap_err("Failed to compress files")?;
        drop(self.writer);
        let output_size = std::fs::metadata(self.path)
            .map(|m| m.len() as f64)
            .unwrap_or(f64::NAN);
        Ok(output_size)
    }
}

pub struct TarCompressor {
    writer: tar::Builder<File>,
    path: PathBuf,
}

impl Compressor for TarCompressor {
    const NAME: &'static str = "tar";

    #[tracing::instrument]
    fn new(path: PathBuf) -> Result<Box<Self>> {
        let writer = tar::Builder::new(File::create(&path).wrap_err("Failed to open file")?);
        Ok(Box::new(Self { writer, path }))
    }

    #[tracing::instrument(skip(self))]
    fn add_file(&mut self, path: &Path, relative_path: &Path) -> Result<f64> {
        let mut file = File::open(path).wrap_err("Failed to open file")?;
        self.writer
            .append_file(relative_path, &mut file)
            .wrap_err("Failed to compress file")?;
        let size = file.metadata().map(|m| m.len() as f64).unwrap_or(f64::NAN);
        Ok(size)
    }

    #[tracing::instrument(skip(self))]
    fn finish(mut self) -> Result<f64> {
        self.writer.finish().wrap_err("Failed to compress files")?;
        drop(self.writer);
        let output_size = std::fs::metadata(self.path)
            .map(|m| m.len() as f64)
            .unwrap_or(f64::NAN);
        Ok(output_size)
    }
}

pub struct CopyCompressor {
    path: PathBuf,
}

impl Compressor for CopyCompressor {
    const NAME: &'static str = "copy";

    fn new(path: PathBuf) -> Result<Box<Self>> {
        if path.exists() {
            bail!("Output path already exists");
        }
        std::fs::create_dir(&path).wrap_err("Failed to create output directory")?;
        Ok(Box::new(Self { path }))
    }

    fn add_file(&mut self, path: &Path, relative_path: &Path) -> Result<f64> {
        let output_path = self.path.join(relative_path);
        std::fs::create_dir_all(output_path.parent().wrap_err("Invalid path")?)
            .wrap_err("Failed to create directory")?;
        let output = std::fs::copy(path, output_path).wrap_err("Failed to copy file")?;
        Ok(output as f64)
    }

    fn finish(self) -> Result<f64> {
        let size = fs_extra::dir::get_size(self.path);
        Ok(size.map(|r| r as f64).unwrap_or(f64::NAN))
    }
}
