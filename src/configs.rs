use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct DolorousConfig {
    pub socket: Option<PathBuf>,
    pub log: String,
    pub tasks: HashMap<String, TaskConfig>,
    pub backups: HashMap<String, BackupsConfig>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
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

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct TaskConfig {
    /// When the task is scheduled. Uses cron syntax.
    pub schedule: String,
    #[serde(deserialize_with = "figment::util::bool_from_str_or_int")]
    pub run_if_stopped: bool,
    pub actions: Vec<ActionType>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", tag = "type")]
pub enum ActionType {
    Backup {
        backup: String,
    },
    Command {
        command: String,
    },
    Start,
    Stop {
        #[serde(flatten)]
        properties: StopProperties,
    },
    Restart {
        #[serde(flatten)]
        properties: StopProperties,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct StopProperties {
    #[serde(default = "default_stop_command")]
    stop_command: String,
    #[serde(with = "humantime_serde", default = "default_duration")]
    term_timeout: Duration,
    #[serde(with = "humantime_serde", default = "default_duration")]
    kill_timeout: Duration,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "kebab-case")]
pub enum BackupFileType {
    Zip,
    TarGz,
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

impl Default for DolorousConfig {
    fn default() -> Self {
        Self {
            socket: Default::default(),
            log: "info".into(),
            tasks: Default::default(),
            backups: Default::default(),
        }
    }
}

impl Default for BackupFileType {
    fn default() -> Self {
        BackupFileType::Zip
    }
}
