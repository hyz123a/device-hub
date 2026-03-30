use device_hub_shared::{LogChannel, LogEvent, LogLevel, LogSource};
use yew::prelude::*;

#[derive(Properties, PartialEq)]
pub struct LogEventCardProps {
    pub event: LogEvent,
}

#[function_component(LogEventCard)]
pub fn log_event_card(props: &LogEventCardProps) -> Html {
    let event = &props.event;
    let message_meta = analyze_message(&event.message);
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
        <div class={classes!(entry_class, "min-w-0", "overflow-hidden")}>
            <div class="mb-3 flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
                <div class="flex flex-wrap items-center gap-2">
                    <span class={channel_class}>{format_channel(&event.channel)}</span>
                    <span class={level_class}>{format!("{:?}", event.level)}</span>
                    <span class="inline-flex rounded-full bg-black/5 px-3 py-1.5 text-xs font-medium text-ink-muted">{format_source(&event.source)}</span>
                </div>
                <span class="font-mono text-sm text-ink-muted">{event.ts.to_rfc3339()}</span>
            </div>
            {
                if message_meta.has_structured_metadata() {
                    html! {
                        <div class="mb-3 flex flex-wrap items-center gap-2 font-mono text-xs text-ink-muted">
                            {
                                if let Some(timestamp) = &message_meta.timestamp {
                                    html! { <span class="inline-flex rounded-full bg-black/5 px-3 py-1.5">{timestamp.clone()}</span> }
                                } else {
                                    Html::default()
                                }
                            }
                            {
                                if let Some(level_hint) = message_meta.level_hint {
                                    html! { <span class={detected_level_class(level_hint)}>{level_hint.label()}</span> }
                                } else {
                                    Html::default()
                                }
                            }
                            {
                                if let Some(module) = &message_meta.module {
                                    html! { <span class="inline-flex rounded-full bg-black/5 px-3 py-1.5">{module.clone()}</span> }
                                } else {
                                    Html::default()
                                }
                            }
                        </div>
                    }
                } else {
                    Html::default()
                }
            }
            <pre class="mb-3 mt-0 min-w-0 overflow-x-auto whitespace-pre-wrap break-all font-mono text-sm text-ink">
                {render_log_message(&event.message, None)}
            </pre>
            <div class="flex flex-wrap gap-2">
                <span class="inline-flex rounded-full bg-black/5 px-3 py-1.5 text-xs font-medium text-ink-muted">{event.device_id.clone()}</span>
                <span class="inline-flex rounded-full bg-black/5 px-3 py-1.5 text-xs font-medium text-ink-muted">{event.job_id.clone()}</span>
            </div>
        </div>
    }
}

pub(crate) fn render_log_message(message: &str, fallback_class: Option<&'static str>) -> Html {
    let message_meta = analyze_message(message);
    let message_segments = parse_ansi_segments(message);
    let body_segments =
        strip_ansi_segments_prefix(&message_segments, message_meta.prefix_chars_to_strip);

    if message_meta.prefer_ansi_render {
        render_ansi_segments(&body_segments)
    } else {
        html! {
            <span class={fallback_class.unwrap_or(detected_body_class(message_meta.level_hint))}>
                {message_meta.body}
            </span>
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct AnsiStyle {
    fg: Option<AnsiColor>,
    bold: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AnsiColor {
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    White,
    BrightBlack,
    BrightRed,
    BrightGreen,
    BrightYellow,
    BrightBlue,
    BrightMagenta,
    BrightCyan,
    BrightWhite,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct AnsiSegment {
    text: String,
    style: AnsiStyle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DetectedLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl DetectedLevel {
    fn label(self) -> &'static str {
        match self {
            Self::Trace => "TRACE",
            Self::Debug => "DEBUG",
            Self::Info => "INFO",
            Self::Warn => "WARN",
            Self::Error => "ERROR",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct MessageMeta {
    timestamp: Option<String>,
    level_hint: Option<DetectedLevel>,
    module: Option<String>,
    body: String,
    prefix_chars_to_strip: usize,
    prefer_ansi_render: bool,
}

impl MessageMeta {
    fn has_structured_metadata(&self) -> bool {
        self.timestamp.is_some() || self.level_hint.is_some() || self.module.is_some()
    }
}

fn render_ansi_segments(segments: &[AnsiSegment]) -> Html {
    html! {
        for segments.iter().map(|segment| {
            html! {
                <span class={ansi_style_class(segment.style)}>{segment.text.clone()}</span>
            }
        })
    }
}

fn strip_ansi_segments_prefix(
    segments: &[AnsiSegment],
    mut chars_to_strip: usize,
) -> Vec<AnsiSegment> {
    if chars_to_strip == 0 {
        return segments.to_vec();
    }

    let mut stripped = Vec::new();
    for segment in segments {
        if chars_to_strip == 0 {
            stripped.push(segment.clone());
            continue;
        }

        let segment_chars = segment.text.chars().count();
        if chars_to_strip >= segment_chars {
            chars_to_strip -= segment_chars;
            continue;
        }

        let text = segment
            .text
            .chars()
            .skip(chars_to_strip)
            .collect::<String>();
        chars_to_strip = 0;
        if !text.is_empty() {
            stripped.push(AnsiSegment {
                text,
                style: segment.style,
            });
        }
    }

    stripped
}

fn ansi_style_class(style: AnsiStyle) -> Classes {
    let mut classes = Classes::from("whitespace-pre-wrap break-all");
    if style.bold {
        classes.push("font-semibold");
    }

    classes.push(match style.fg {
        Some(AnsiColor::Black) => "text-slate-900",
        Some(AnsiColor::Red) => "text-red-700",
        Some(AnsiColor::Green) => "text-emerald-700",
        Some(AnsiColor::Yellow) => "text-amber-700",
        Some(AnsiColor::Blue) => "text-blue-700",
        Some(AnsiColor::Magenta) => "text-fuchsia-700",
        Some(AnsiColor::Cyan) => "text-cyan-700",
        Some(AnsiColor::White) => "text-slate-200",
        Some(AnsiColor::BrightBlack) => "text-slate-500",
        Some(AnsiColor::BrightRed) => "text-red-500",
        Some(AnsiColor::BrightGreen) => "text-emerald-500",
        Some(AnsiColor::BrightYellow) => "text-amber-500",
        Some(AnsiColor::BrightBlue) => "text-blue-500",
        Some(AnsiColor::BrightMagenta) => "text-fuchsia-500",
        Some(AnsiColor::BrightCyan) => "text-cyan-500",
        Some(AnsiColor::BrightWhite) => "text-slate-50",
        None => "text-inherit",
    });

    classes
}

fn detected_level_class(level: DetectedLevel) -> &'static str {
    match level {
        DetectedLevel::Trace => "inline-flex rounded-full bg-black/5 px-3 py-1.5 text-xs font-medium text-slate-500",
        DetectedLevel::Debug => "inline-flex rounded-full bg-slate-500/15 px-3 py-1.5 text-xs font-medium text-slate-700",
        DetectedLevel::Info => "inline-flex rounded-full bg-cyan-500/15 px-3 py-1.5 text-xs font-medium text-cyan-700",
        DetectedLevel::Warn => "inline-flex rounded-full bg-amber-500/15 px-3 py-1.5 text-xs font-medium text-amber-800",
        DetectedLevel::Error => "inline-flex rounded-full bg-red-700/10 px-3 py-1.5 text-xs font-medium text-red-700",
    }
}

fn detected_body_class(level: Option<DetectedLevel>) -> &'static str {
    match level {
        Some(DetectedLevel::Trace) => "whitespace-pre-wrap break-all text-slate-500",
        Some(DetectedLevel::Debug) => "whitespace-pre-wrap break-all text-slate-700",
        Some(DetectedLevel::Info) => "whitespace-pre-wrap break-all text-cyan-800",
        Some(DetectedLevel::Warn) => "whitespace-pre-wrap break-all text-amber-800",
        Some(DetectedLevel::Error) => "whitespace-pre-wrap break-all text-red-700",
        None => "whitespace-pre-wrap break-all text-inherit",
    }
}

fn analyze_message(message: &str) -> MessageMeta {
    let segments = parse_ansi_segments(message);
    let plain = segments
        .iter()
        .map(|segment| segment.text.as_str())
        .collect::<String>();
    let prefer_ansi_render = segments
        .iter()
        .any(|segment| segment.style != AnsiStyle::default());
    let trimmed = plain.trim();
    let leading_trim_chars = plain.chars().take_while(|ch| ch.is_whitespace()).count();

    let (timestamp, after_timestamp) = consume_timestamp(trimmed);
    let (level_hint, after_level) = consume_level(after_timestamp);
    let (module, body) = consume_module(after_level);
    let body = body
        .trim_start_matches([' ', ':', '-', '|'])
        .trim()
        .to_owned();
    let prefix_chars_to_strip = if body.is_empty() {
        0
    } else {
        leading_trim_chars + trimmed.chars().count().saturating_sub(body.chars().count())
    };

    MessageMeta {
        timestamp,
        level_hint,
        module,
        body: if body.is_empty() { plain } else { body },
        prefix_chars_to_strip,
        prefer_ansi_render,
    }
}

fn parse_ansi_segments(message: &str) -> Vec<AnsiSegment> {
    let mut segments = Vec::new();
    let mut style = AnsiStyle::default();
    let mut buffer = String::new();
    let bytes = message.as_bytes();
    let mut idx = 0;

    while idx < bytes.len() {
        if bytes[idx] == 0x1b && idx + 1 < bytes.len() && bytes[idx + 1] == b'[' {
            if let Some((codes, next_idx)) = parse_sgr_codes(bytes, idx + 2) {
                if !buffer.is_empty() {
                    segments.push(AnsiSegment {
                        text: std::mem::take(&mut buffer),
                        style,
                    });
                }
                apply_sgr_codes(&mut style, &codes);
                idx = next_idx;
                continue;
            }
        }

        if let Some(ch) = message[idx..].chars().next() {
            buffer.push(ch);
            idx += ch.len_utf8();
        } else {
            break;
        }
    }

    if !buffer.is_empty() || segments.is_empty() {
        segments.push(AnsiSegment {
            text: buffer,
            style,
        });
    }

    segments
}

fn consume_timestamp(input: &str) -> (Option<String>, &str) {
    let mut parts = input.splitn(3, ' ');
    let first = parts.next().unwrap_or_default();
    let second = parts.next().unwrap_or_default();

    if looks_like_date(first) && looks_like_time(second) {
        let consumed = first.len() + 1 + second.len();
        return (
            Some(format!("{first} {second}")),
            input[consumed..].trim_start(),
        );
    }

    if looks_like_time(first) {
        return (Some(first.to_owned()), input[first.len()..].trim_start());
    }

    (None, input)
}

fn looks_like_date(value: &str) -> bool {
    value.len() >= 8
        && value
            .chars()
            .all(|ch| ch.is_ascii_digit() || matches!(ch, '-' | '/'))
        && value.chars().filter(|&ch| matches!(ch, '-' | '/')).count() >= 2
}

fn looks_like_time(value: &str) -> bool {
    let core = value.trim_matches(|ch| matches!(ch, '[' | ']'));
    core.chars()
        .all(|ch| ch.is_ascii_digit() || matches!(ch, ':' | '.' | ','))
        && core.chars().filter(|&ch| ch == ':').count() >= 1
}

fn consume_level(input: &str) -> (Option<DetectedLevel>, &str) {
    const LEVEL_PATTERNS: [(&str, DetectedLevel); 15] = [
        ("[TRACE]", DetectedLevel::Trace),
        ("[DEBUG]", DetectedLevel::Debug),
        ("[INFO]", DetectedLevel::Info),
        ("[WARN]", DetectedLevel::Warn),
        ("[WARNING]", DetectedLevel::Warn),
        ("[ERROR]", DetectedLevel::Error),
        ("[T]", DetectedLevel::Trace),
        ("[D]", DetectedLevel::Debug),
        ("[I]", DetectedLevel::Info),
        ("[W]", DetectedLevel::Warn),
        ("[E]", DetectedLevel::Error),
        ("TRACE", DetectedLevel::Trace),
        ("DEBUG", DetectedLevel::Debug),
        ("INFO", DetectedLevel::Info),
        ("WARN", DetectedLevel::Warn),
    ];

    let input = input.trim_start();
    let upper = input.to_ascii_uppercase();
    for (pattern, level) in LEVEL_PATTERNS {
        if upper.starts_with(pattern) {
            let rest = input[pattern.len()..]
                .trim_start_matches([' ', ':', '-', '|'])
                .trim_start();
            return (Some(level), rest);
        }
    }

    if upper.starts_with("ERROR") {
        let rest = input[5..]
            .trim_start_matches([' ', ':', '-', '|'])
            .trim_start();
        return (Some(DetectedLevel::Error), rest);
    }

    (None, input)
}

fn consume_module(input: &str) -> (Option<String>, &str) {
    let trimmed = input.trim_start();
    let Some(separator_idx) = trimmed.find(':') else {
        return (None, trimmed);
    };

    let candidate = trimmed[..separator_idx].trim();
    if candidate.is_empty() || candidate.len() > 80 || candidate.contains(' ') {
        return (None, trimmed);
    }

    if candidate.chars().all(|ch| {
        ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | ':' | '.' | '(' | ')' | '[' | ']')
    }) {
        let body = trimmed[separator_idx + 1..].trim_start();
        return (Some(candidate.to_owned()), body);
    }

    (None, trimmed)
}

fn parse_sgr_codes(bytes: &[u8], start: usize) -> Option<(Vec<u16>, usize)> {
    let mut idx = start;
    while idx < bytes.len() {
        match bytes[idx] {
            b'0'..=b'9' | b';' => idx += 1,
            b'm' => {
                let raw = std::str::from_utf8(&bytes[start..idx]).ok()?;
                let codes = if raw.is_empty() {
                    vec![0]
                } else {
                    raw.split(';')
                        .map(|part| part.parse::<u16>().ok())
                        .collect::<Option<Vec<_>>>()?
                };
                return Some((codes, idx + 1));
            }
            _ => return None,
        }
    }

    None
}

fn apply_sgr_codes(style: &mut AnsiStyle, codes: &[u16]) {
    for code in codes {
        match code {
            0 => *style = AnsiStyle::default(),
            1 => style.bold = true,
            22 => style.bold = false,
            30 => style.fg = Some(AnsiColor::Black),
            31 => style.fg = Some(AnsiColor::Red),
            32 => style.fg = Some(AnsiColor::Green),
            33 => style.fg = Some(AnsiColor::Yellow),
            34 => style.fg = Some(AnsiColor::Blue),
            35 => style.fg = Some(AnsiColor::Magenta),
            36 => style.fg = Some(AnsiColor::Cyan),
            37 => style.fg = Some(AnsiColor::White),
            39 => style.fg = None,
            90 => style.fg = Some(AnsiColor::BrightBlack),
            91 => style.fg = Some(AnsiColor::BrightRed),
            92 => style.fg = Some(AnsiColor::BrightGreen),
            93 => style.fg = Some(AnsiColor::BrightYellow),
            94 => style.fg = Some(AnsiColor::BrightBlue),
            95 => style.fg = Some(AnsiColor::BrightMagenta),
            96 => style.fg = Some(AnsiColor::BrightCyan),
            97 => style.fg = Some(AnsiColor::BrightWhite),
            _ => {}
        }
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
        LogSource::Serial => "serial",
        LogSource::Jlink => "jlink",
        LogSource::System => "system",
        LogSource::Job => "job",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_plain_text_as_single_segment() {
        let segments = parse_ansi_segments("plain text");
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].text, "plain text");
        assert_eq!(segments[0].style, AnsiStyle::default());
    }

    #[test]
    fn parses_basic_ansi_colors() {
        let segments = parse_ansi_segments("\u{1b}[33mWARN\u{1b}[0m ok");
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].text, "WARN");
        assert_eq!(segments[0].style.fg, Some(AnsiColor::Yellow));
        assert_eq!(segments[1].text, " ok");
        assert_eq!(segments[1].style, AnsiStyle::default());
    }

    #[test]
    fn detects_bracket_short_level_prefix() {
        let meta = analyze_message("[I] boot completed");
        assert_eq!(meta.level_hint, Some(DetectedLevel::Info));
        assert_eq!(meta.body, "boot completed");
    }

    #[test]
    fn detects_timestamp_level_and_module() {
        let meta = analyze_message("2026-03-30 09:26:12.769 INFO main.loop: boot completed");
        assert_eq!(meta.timestamp.as_deref(), Some("2026-03-30 09:26:12.769"));
        assert_eq!(meta.level_hint, Some(DetectedLevel::Info));
        assert_eq!(meta.module.as_deref(), Some("main.loop"));
        assert_eq!(meta.body, "boot completed");
    }

    #[test]
    fn strips_detected_prefix_from_ansi_segments() {
        let segments = parse_ansi_segments("\u{1b}[36m[I]\u{1b}[0m boot completed");
        let meta = analyze_message("\u{1b}[36m[I]\u{1b}[0m boot completed");
        let body_segments = strip_ansi_segments_prefix(&segments, meta.prefix_chars_to_strip);
        let body = body_segments
            .iter()
            .map(|segment| segment.text.as_str())
            .collect::<String>();

        assert_eq!(meta.level_hint, Some(DetectedLevel::Info));
        assert_eq!(body, "boot completed");
    }
}
