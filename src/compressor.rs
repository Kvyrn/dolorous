use color_eyre::eyre::{eyre, WrapErr};
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
            relative_path.to_str().ok_or_else(|| eyre!("Invalid file name"))?,
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
        self.writer.append_file(relative_path, &mut file).wrap_err("Failed to compress file")?;
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
