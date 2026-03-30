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
        LogLevel::Error => "inline-flex rounded-full bg-red-700/10 px-3 py-1.5 text-xs font-medium text-red-700",
        LogLevel::Warn => "inline-flex rounded-full bg-amber-500/15 px-3 py-1.5 text-xs font-medium text-amber-800",
        LogLevel::Info => "inline-flex rounded-full bg-accent/10 px-3 py-1.5 text-xs font-medium text-accent-strong",
        LogLevel::Debug => "inline-flex rounded-full bg-black/5 px-3 py-1.5 text-xs font-medium text-ink-muted",
    };
    let channel_class = match event.channel {
        LogChannel::Stdout => "inline-flex rounded-full bg-emerald-500/15 px-3 py-1.5 text-xs font-medium text-emerald-700",
        LogChannel::Stderr => "inline-flex rounded-full bg-red-700/10 px-3 py-1.5 text-xs font-medium text-red-700",
        LogChannel::System => "inline-flex rounded-full bg-slate-500/15 px-3 py-1.5 text-xs font-medium text-slate-700",
    };
    let entry_class = classes!(
        "border-l-4",
        "border-t",
        "border-black/10",
        "pl-4",
        "pt-4",
        "first:border-t-0",
        "first:pt-0",
        match event.channel {
            LogChannel::Stdout => "border-l-emerald-600",
            LogChannel::Stderr => "border-l-red-700",
            LogChannel::System => "border-l-slate-600",
        }
    );

    html! {
        <div class={entry_class}>
            <div class="mb-3 flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
                <div class="flex flex-wrap items-center gap-2">
                    <span class={channel_class}>{format_channel(&event.channel)}</span>
                    <span class={level_class}>{format!("{:?}", event.level)}</span>
                    <span class="inline-flex rounded-full bg-black/5 px-3 py-1.5 text-xs font-medium text-ink-muted">{format_source(&event.source)}</span>
                </div>
                <span class="font-mono text-sm text-ink-muted">{event.ts.to_rfc3339()}</span>
            </div>
            <pre class="mb-3 mt-0 whitespace-pre-wrap break-words font-mono text-sm text-ink">{event.message.clone()}</pre>
            <div class="flex flex-wrap gap-2">
                <span class="inline-flex rounded-full bg-black/5 px-3 py-1.5 text-xs font-medium text-ink-muted">{event.device_id.clone()}</span>
                <span class="inline-flex rounded-full bg-black/5 px-3 py-1.5 text-xs font-medium text-ink-muted">{event.job_id.clone()}</span>
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
