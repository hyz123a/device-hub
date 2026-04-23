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
    pub source: Option<LogSource>,
    pub level: Option<LogLevel>,
    pub search: Option<String>,
    pub structured_only: Option<bool>,
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
        let source_ok = self
            .source
            .as_ref()
            .is_none_or(|source| source == &event.source);
        let level_ok = self
            .level
            .as_ref()
            .is_none_or(|level| level == &event.level);
        let structured_ok = self.structured_only.is_none_or(|structured_only| {
            event_is_structured(event) == structured_only
        });
        let search_ok = self
            .search
            .as_deref()
            .is_none_or(|query| event_matches_search_query(event, query));

        device_ok && job_ok && since_ok && source_ok && level_ok && structured_ok && search_ok
    }
}

fn event_is_structured(event: &LogEvent) -> bool {
    matches!(event.source, LogSource::System | LogSource::Job)
        || matches!(event.channel, LogChannel::System)
}

fn event_matches_search_query(event: &LogEvent, query: &str) -> bool {
    [
        event.message.as_str(),
        event.device_id.as_str(),
        event.job_id.as_str(),
        log_source_label(&event.source),
        log_channel_label(&event.channel),
        log_level_label(&event.level),
    ]
    .into_iter()
    .any(|value| value.to_ascii_lowercase().contains(query))
}

fn log_source_label(source: &LogSource) -> &'static str {
    match source {
        LogSource::Ssh => "ssh",
        LogSource::Serial => "serial",
        LogSource::Jlink => "jlink",
        LogSource::System => "system",
        LogSource::Job => "job",
    }
}

fn log_channel_label(channel: &LogChannel) -> &'static str {
    match channel {
        LogChannel::Stdout => "stdout",
        LogChannel::Stderr => "stderr",
        LogChannel::System => "system",
    }
}

fn log_level_label(level: &LogLevel) -> &'static str {
    match level {
        LogLevel::Debug => "debug",
        LogLevel::Info => "info",
        LogLevel::Warn => "warn",
        LogLevel::Error => "error",
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LogSource {
    Ssh,
    Serial,
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
