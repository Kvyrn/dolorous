use async_compression::tokio::write::GzipEncoder;
use async_compression::Level;
use async_trait::async_trait;
use async_zip::write::ZipFileWriter;
use async_zip::ZipEntryBuilder;
use color_eyre::eyre::{bail, eyre, ContextCompat, WrapErr};
use color_eyre::Result;
use std::path::{Path, PathBuf};
use tokio::fs::File;

#[async_trait]
pub trait Compressor {
    const NAME: &'static str;
    async fn new(path: PathBuf) -> Result<Box<Self>>;
    /// Returns: size of original size
    async fn add_file(&mut self, path: &Path, relative_path: &Path) -> Result<f64>;
    /// Returns: size of compressed file
    async fn finish(self) -> Result<f64>;
}

pub struct ZipCompressor {
    writer: ZipFileWriter<File>,
    path: PathBuf,
}

#[async_trait]
impl Compressor for ZipCompressor {
    const NAME: &'static str = "zip";

    #[tracing::instrument]
    async fn new(path: PathBuf) -> Result<Box<Self>> {
        let writer = ZipFileWriter::new(
            File::create(&path)
                .await
                .wrap_err("Failed to create output file!")?,
        );
        Ok(Box::new(Self { writer, path }))
    }

    #[tracing::instrument(skip(self))]
    async fn add_file(&mut self, path: &Path, relative_path: &Path) -> Result<f64> {
        // TODO: more compressions
        let builder = ZipEntryBuilder::new(
            relative_path
                .to_str()
                .ok_or_else(|| eyre!("Invalid file name"))?
                .to_string(),
            async_zip::Compression::Deflate,
        );
        let mut stream_writer = self.writer.write_entry_stream(builder).await?;
        let mut input_file = File::open(path).await.wrap_err("Failed to open file")?;
        let compressed = tokio::io::copy(&mut input_file, &mut stream_writer)
            .await
            .wrap_err("Failed to compress file!")?;
        Ok(compressed as f64)
    }

    #[tracing::instrument(skip(self))]
    async fn finish(mut self) -> Result<f64> {
        self.writer
            .close()
            .await
            .wrap_err("Failed to compress files")?;
        let output_size = tokio::fs::metadata(&self.path)
            .await
            .map(|r| r.len() as f64)
            .unwrap_or(f64::NAN);
        Ok(output_size)
    }
}

pub struct TarGzCompressor<const LEVEL: u32> {
    writer: tokio_tar::Builder<GzipEncoder<File>>,
    path: PathBuf,
}

#[async_trait]
impl<const LEVEL: u32> Compressor for TarGzCompressor<LEVEL> {
    const NAME: &'static str = "targz";

    #[tracing::instrument]
    async fn new(path: PathBuf) -> Result<Box<Self>> {
        let compressor = GzipEncoder::with_quality(
            File::create(&path).await.wrap_err("Failed to open file")?,
            Level::Precise(LEVEL),
        );
        let writer = tokio_tar::Builder::new(compressor);
        Ok(Box::new(Self { writer, path }))
    }

    #[tracing::instrument(skip(self))]
    async fn add_file(&mut self, path: &Path, relative_path: &Path) -> Result<f64> {
        let mut file = File::open(path).await.wrap_err("Failed to open file")?;
        self.writer
            .append_file(relative_path, &mut file)
            .await
            .wrap_err("Failed to compress file")?;
        let size = file
            .metadata()
            .await
            .map(|m| m.len() as f64)
            .unwrap_or(f64::NAN);
        Ok(size)
    }

    #[tracing::instrument(skip(self))]
    async fn finish(mut self) -> Result<f64> {
        self.writer
            .finish()
            .await
            .wrap_err("Failed to compress files")?;
        drop(self.writer);
        let output_size = tokio::fs::metadata(self.path)
            .await
            .map(|m| m.len() as f64)
            .unwrap_or(f64::NAN);
        Ok(output_size)
    }
}

pub struct TarCompressor {
    writer: tokio_tar::Builder<File>,
    path: PathBuf,
}

#[async_trait]
impl Compressor for TarCompressor {
    const NAME: &'static str = "tar";

    #[tracing::instrument]
    async fn new(path: PathBuf) -> Result<Box<Self>> {
        let writer =
            tokio_tar::Builder::new(File::create(&path).await.wrap_err("Failed to open file")?);
        Ok(Box::new(Self { writer, path }))
    }

    #[tracing::instrument(skip(self))]
    async fn add_file(&mut self, path: &Path, relative_path: &Path) -> Result<f64> {
        let mut file = File::open(path).await.wrap_err("Failed to open file")?;
        self.writer
            .append_file(relative_path, &mut file)
            .await
            .wrap_err("Failed to compress file")?;
        let size = file
            .metadata()
            .await
            .map(|m| m.len() as f64)
            .unwrap_or(f64::NAN);
        Ok(size)
    }

    #[tracing::instrument(skip(self))]
    async fn finish(mut self) -> Result<f64> {
        self.writer
            .finish()
            .await
            .wrap_err("Failed to compress files")?;
        drop(self.writer);
        let output_size = tokio::fs::metadata(self.path)
            .await
            .map(|m| m.len() as f64)
            .unwrap_or(f64::NAN);
        Ok(output_size)
    }
}

pub struct CopyCompressor {
    path: PathBuf,
}

#[async_trait]
impl Compressor for CopyCompressor {
    const NAME: &'static str = "copy";

    #[tracing::instrument]
    async fn new(path: PathBuf) -> Result<Box<Self>> {
        if path.exists() {
            bail!("Output path already exists");
        }
        tokio::fs::create_dir(&path)
            .await
            .wrap_err("Failed to create output directory")?;
        Ok(Box::new(Self { path }))
    }

    #[tracing::instrument(skip(self))]
    async fn add_file(&mut self, path: &Path, relative_path: &Path) -> Result<f64> {
        let output_path = self.path.join(relative_path);
        tokio::fs::create_dir_all(output_path.parent().wrap_err("Invalid path")?)
            .await
            .wrap_err("Failed to create directory")?;
        let output = tokio::fs::copy(path, output_path)
            .await
            .wrap_err("Failed to copy file")?;
        Ok(output as f64)
    }

    #[tracing::instrument(skip(self))]
    async fn finish(self) -> Result<f64> {
        let size = fs_extra::dir::get_size(self.path);
        Ok(size.map(|r| r as f64).unwrap_or(f64::NAN))
    }
}
