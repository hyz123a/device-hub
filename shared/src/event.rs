use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct LogEvent {
    pub ts: DateTime<Utc>,
    pub device_id: String,
    pub job_id: String,
    pub source: LogSource,
    pub channel: LogChannel,
    pub level: LogLevel,
    pub message: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct EventFilter {
    pub device_id: Option<String>,
    pub job_id: Option<String>,
    pub since: Option<DateTime<Utc>>,
}

impl EventFilter {
    pub fn matches(&self, event: &LogEvent) -> bool {
        let device_ok = self
            .device_id
            .as_ref()
            .is_none_or(|device_id| device_id == &event.device_id);
        let job_ok = self
            .job_id
            .as_ref()
            .is_none_or(|job_id| job_id == &event.job_id);
        let since_ok = self.since.is_none_or(|since| event.ts >= since);

        device_ok && job_ok && since_ok
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LogSource {
    Ssh,
    Jlink,
    System,
    Job,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LogChannel {
    Stdout,
    Stderr,
    System,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}
