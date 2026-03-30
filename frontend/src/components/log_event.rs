use device_hub_shared::{LogChannel, LogEvent, LogLevel, LogSource};
use yew::prelude::*;

#[derive(Properties, PartialEq)]
pub struct LogEventCardProps {
    pub event: LogEvent,
}

#[function_component(LogEventCard)]
pub fn log_event_card(props: &LogEventCardProps) -> Html {
    let event = &props.event;
    let level_class = match event.level {
        LogLevel::Error => "status status-error",
        LogLevel::Warn => "status status-warn",
        LogLevel::Info => "status status-ok",
        LogLevel::Debug => "status status-muted",
    };
    let channel_class = match event.channel {
        LogChannel::Stdout => "status status-stdout",
        LogChannel::Stderr => "status status-stderr",
        LogChannel::System => "status status-system",
    };
    let entry_class = classes!(
        "log-entry",
        match event.channel {
            LogChannel::Stdout => "log-entry-stdout",
            LogChannel::Stderr => "log-entry-stderr",
            LogChannel::System => "log-entry-system",
        }
    );

    html! {
        <div class={entry_class}>
            <div class="log-entry-head">
                <div class="log-entry-summary">
                    <span class={channel_class}>{format_channel(&event.channel)}</span>
                    <span class={level_class}>{format!("{:?}", event.level)}</span>
                    <span class="status status-muted">{format_source(&event.source)}</span>
                </div>
                <span class="mono muted">{event.ts.to_rfc3339()}</span>
            </div>
            <pre class="log-message mono">{event.message.clone()}</pre>
            <div class="log-tags">
                <span class="status status-muted">{event.device_id.clone()}</span>
                <span class="status status-muted">{event.job_id.clone()}</span>
            </div>
        </div>
    }
}

fn format_channel(channel: &LogChannel) -> &'static str {
    match channel {
        LogChannel::Stdout => "stdout",
        LogChannel::Stderr => "stderr",
        LogChannel::System => "system",
    }
}

fn format_source(source: &LogSource) -> &'static str {
    match source {
        LogSource::Ssh => "ssh",
        LogSource::Jlink => "jlink",
        LogSource::System => "system",
        LogSource::Job => "job",
    }
}
