use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct DolorousConfig {
    pub socket: Option<PathBuf>,
    #[serde(default = "default_log_filter")]
    pub log_filter: String,
    pub process: ProcessConfig,
    pub tasks: HashMap<String, TaskConfig>,
    pub backups: HashMap<String, BackupsConfig>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct ProcessConfig {
    pub command: String,
    #[serde(default = "default_cache_size")]
    pub cache_size: u32,
    pub restart: RestartCondition,
    pub stop_config: StopProperties,
    #[cfg_attr(feature = "docker", serde(default = "default_wroking_directory"))]
    pub working_directory: PathBuf,
    #[serde(default = "default_restart_attempts")]
    pub restart_attempts: u16,
    #[serde(with = "humantime_serde", default = "default_restart_delay")]
    pub restart_delay: Duration,
    /// Delay after witch the startup is considered done. Restart attempt counter is reset.
    #[serde(with = "humantime_serde", default = "default_watch_delay")]
    pub watch_delay: Duration,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct BackupsConfig {
    pub output: PathBuf,
    pub location: PathBuf,
    #[serde(default = "default_time_format")]
    pub time_format: String,
    #[serde(default = "default_name")]
    pub name: String,
    #[serde(default)]
    pub file_type: BackupFileType,
    pub files: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "kebab-case")]
pub struct TaskConfig {
    /// When the task is scheduled. Uses cron syntax.
    pub schedule: String,
    pub run_if_stopped: bool,
    pub actions: Vec<ActionType>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "kebab-case", tag = "type")]
pub enum ActionType {
    Backup { backup: String },
    Command { command: String },
    Start,
    Stop,
    Restart,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "kebab-case")]
pub struct StopProperties {
    #[serde(default = "default_stop_command")]
    pub stop_command: String,
    #[serde(with = "humantime_serde", default = "default_duration")]
    pub term_timeout: Duration,
    #[serde(with = "humantime_serde", default = "default_duration")]
    pub kill_timeout: Duration,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BackupFileType {
    Zip,
    TarGz,
    TarGzFast,
    TarGzSmall,
    Tar,
    Copy,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RestartCondition {
    Never,
    /// Zero exit code
    UnlessCrashed,
    /// Non-zero exit code
    IfCrashed,
    Always,
}

fn default_duration() -> Duration {
    Duration::from_secs(180)
}

fn default_stop_command() -> String {
    "stop".into()
}

fn default_time_format() -> String {
    "%Y%m%d-%H".into()
}

fn default_name() -> String {
    "{date}.{extension}".into()
}

fn default_log_filter() -> String {
    "info".into()
}

fn default_cache_size() -> u32 {
    // 8KiB
    2u32.pow(10) * 8
}

fn default_restart_attempts() -> u16 {
    5
}

fn default_restart_delay() -> Duration {
    Duration::from_secs(30)
}

/// Default server working directory for docker containers
#[cfg(feature = "docker")]
fn default_wroking_directory() -> PathBuf {
    PathBuf::from("/server")
}

fn default_watch_delay() -> Duration {
    Duration::from_secs(60)
}

impl Default for BackupFileType {
    fn default() -> Self {
        Self::Zip
    }
}
