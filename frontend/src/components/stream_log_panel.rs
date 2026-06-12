use std::collections::HashSet;

use device_hub_shared::{LogChannel, LogEvent, LogLevel, LogSource};
use gloo_timers::callback::Timeout;
use wasm_bindgen::{JsCast, closure::Closure};
use wasm_bindgen_futures::spawn_local;
use web_sys::{ErrorEvent, Event, HtmlInputElement, HtmlSelectElement, MessageEvent, WebSocket};
use yew::prelude::*;

use crate::api;
use crate::components::{
    log_event::LogEventCard, serial_write_panel::SerialWritePanel, xterm_terminal::XtermTerminal,
};

const EVENT_PAGE_SIZE: usize = 50;
const DEFAULT_BUFFERED_EVENTS: usize = 100_000;
const MAX_BUFFERED_EVENTS: usize = 1_000_000;
const BUFFERED_EVENT_LIMIT_OPTIONS: [usize; 4] = [100_000, 200_000, 500_000, 1_000_000];
const RENDER_FLUSH_DELAY_MS: u32 = 32;

struct WsBindings {
    socket: WebSocket,
    _on_open: Closure<dyn FnMut(Event)>,
    _on_message: Closure<dyn FnMut(MessageEvent)>,
    _on_error: Closure<dyn FnMut(ErrorEvent)>,
    _on_close: Closure<dyn FnMut(Event)>,
}

impl Drop for WsBindings {
    fn drop(&mut self) {
        let _ = self.socket.close();
        self.socket.set_onopen(None);
        self.socket.set_onmessage(None);
        self.socket.set_onerror(None);
        self.socket.set_onclose(None);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LogViewMode {
    Event,
    Stream,
}

#[derive(Clone, PartialEq, Eq)]
enum StreamFilter {
    All,
    DeviceLogs,
    Structured,
    Source(LogSource),
}

#[derive(Properties, PartialEq)]
pub struct StreamLogPanelProps {
    /// Pre-loaded historical events (e.g. from GET /jobs/:id/events). The
    /// panel merges these with live WS events on mount.
    #[prop_or_default]
    pub initial_events: Vec<LogEvent>,
    /// WS URL to subscribe to. When `None`, the panel skips the WS connection.
    #[prop_or_default]
    pub ws_url: Option<String>,
    /// Device id for the serial write sub-panel. `None` hides the panel.
    #[prop_or_default]
    pub serial_write_device_id: Option<String>,
    /// Revision counter -- bump to force a reconnect.
    #[prop_or_default]
    pub connect_revision: u32,
    /// Externally-driven connection status label (e.g. when the parent
    /// controls the WS lifecycle). When `None` the panel manages its own.
    #[prop_or_default]
    pub connection_status: Option<String>,
    /// Called whenever the internal connection status changes.
    #[prop_or_default]
    pub on_connection_change: Callback<String>,
    /// Called whenever a WS parse error occurs.
    #[prop_or_default]
    pub on_error: Callback<String>,
    /// Shows an operator control that clears the server-side in-memory event cache.
    #[prop_or_default]
    pub show_clear_cache: bool,
}

#[function_component(StreamLogPanel)]
pub fn stream_log_panel(props: &StreamLogPanelProps) -> Html {
    let events = use_state(Vec::<LogEvent>::new);
    let buffered_event_limit = use_state(|| DEFAULT_BUFFERED_EVENTS);
    let current_page = use_state(|| 0_usize);
    let event_store = use_mut_ref(Vec::<LogEvent>::new);
    let dedup_set = use_mut_ref(HashSet::<u64>::new);
    let pending_render = use_mut_ref(|| Option::<Timeout>::None);
    let search_query = use_state(String::new);
    let debounced_search_query = use_state(String::new);
    let pending_search_sync = use_mut_ref(|| Option::<Timeout>::None);
    let internal_connection = use_state(|| "connecting".to_owned());
    let view_mode = use_state(|| LogViewMode::Stream);
    let stream_filter = use_state(|| StreamFilter::DeviceLogs);
    let scroll_to_bottom_revision = use_state(|| 0_u32);
    let stream_at_bottom = use_state(|| true);
    let clear_cache_status = use_state(|| Option::<String>::None);
    let clear_cache_pending = use_state(|| false);
    let socket = use_mut_ref(|| Option::<WsBindings>::None);

    let connection = props
        .connection_status
        .clone()
        .unwrap_or_else(|| (*internal_connection).clone());

    // Seed store with initial events on mount / revision change
    {
        let events = events.clone();
        let event_store = event_store.clone();
        let dedup_set = dedup_set.clone();
        let buffered_event_limit = buffered_event_limit.clone();
        let initial_events = props.initial_events.clone();
        let current_page = current_page.clone();
        let pending_render = pending_render.clone();
        let connect_revision = props.connect_revision;
        use_effect_with((connect_revision, initial_events.len()), move |_| {
            let mut store = event_store.borrow_mut();
            let mut set = dedup_set.borrow_mut();
            store.clear();
            set.clear();
            pending_render.borrow_mut().take();
            for event in &initial_events {
                let hash = event_identity_hash(event);
                if set.insert(hash) {
                    store.push(event.clone());
                }
            }
            store.sort_by(|a, b| a.ts.cmp(&b.ts));
            trim_buffered_events(&mut store, MAX_BUFFERED_EVENTS);
            let visible = visible_buffered_events(&store, *buffered_event_limit);
            events.set(visible);
            current_page.set(0);
            || {}
        });
    }

    {
        let debounced_search_query = debounced_search_query.clone();
        let pending_search_sync = pending_search_sync.clone();
        let search_query_value = (*search_query).clone();
        use_effect_with(search_query_value, move |search_query_value| {
            pending_search_sync.borrow_mut().take();
            let debounced_search_query = debounced_search_query.clone();
            let next = search_query_value.clone();
            *pending_search_sync.borrow_mut() = Some(Timeout::new(250, move || {
                debounced_search_query.set(next.clone());
            }));
            let pending_search_sync = pending_search_sync.clone();
            move || {
                pending_search_sync.borrow_mut().take();
            }
        });
    }

    let effective_ws_url = use_memo(
        (
            props.ws_url.clone(),
            (*stream_filter).clone(),
            (*debounced_search_query).clone(),
        ),
        |(base_ws_url, stream_filter, debounced_search_query)| {
            append_stream_ws_query(
                base_ws_url.clone(),
                stream_filter,
                debounced_search_query.as_str(),
            )
        },
    );

    // WebSocket lifecycle
    {
        let events = events.clone();
        let event_store = event_store.clone();
        let dedup_set = dedup_set.clone();
        let internal_connection = internal_connection.clone();
        let on_connection_change = props.on_connection_change.clone();
        let on_error_cb = props.on_error.clone();
        let buffered_event_limit = buffered_event_limit.clone();
        let pending_render = pending_render.clone();
        let socket = socket.clone();
        let ws_url = (*effective_ws_url).clone();
        let connect_revision = props.connect_revision;
        use_effect_with((connect_revision, ws_url.clone()), move |_| {
            if let Some(existing) = socket.borrow_mut().take() {
                let _ = existing.socket.close();
            }

            let set_connection = {
                let internal_connection = internal_connection.clone();
                let on_connection_change = on_connection_change.clone();
                move |status: &str| {
                    internal_connection.set(status.to_owned());
                    on_connection_change.emit(status.to_owned());
                }
            };

            if let Some(url) = ws_url {
                set_connection("connecting");

                match WebSocket::new(&url) {
                    Ok(ws) => {
                        let set_open = {
                            let internal_connection = internal_connection.clone();
                            let on_connection_change = on_connection_change.clone();
                            move || {
                                internal_connection.set("connected".to_owned());
                                on_connection_change.emit("connected".to_owned());
                            }
                        };
                        let on_open = Closure::wrap(Box::new(move |_event: Event| {
                            set_open();
                        })
                            as Box<dyn FnMut(_)>);
                        ws.set_onopen(Some(on_open.as_ref().unchecked_ref()));

                        let message_events = events.clone();
                        let message_store = event_store.clone();
                        let message_dedup = dedup_set.clone();
                        let message_error = on_error_cb.clone();
                        let buffered_event_limit = buffered_event_limit.clone();
                        let pending_render = pending_render.clone();
                        let on_message = Closure::wrap(Box::new(move |event: MessageEvent| {
                            if let Some(text) = event.data().as_string() {
                                match serde_json::from_str::<LogEvent>(&text) {
                                    Ok(log_event) => {
                                        let hash = event_identity_hash(&log_event);
                                        if !message_dedup.borrow_mut().insert(hash) {
                                            return;
                                        }
                                        let mut next = message_store.borrow_mut();
                                        next.push(log_event);
                                        trim_buffered_events(&mut next, MAX_BUFFERED_EVENTS);
                                        if pending_render.borrow().is_none() {
                                            let message_events = message_events.clone();
                                            let message_store = message_store.clone();
                                            let buffered_event_limit = buffered_event_limit.clone();
                                            let pending_render_for_timer = pending_render.clone();
                                            let pending_render_for_callback =
                                                pending_render.clone();
                                            *pending_render_for_timer.borrow_mut() = Some(
                                                Timeout::new(RENDER_FLUSH_DELAY_MS, move || {
                                                    let snapshot = visible_buffered_events(
                                                        message_store.borrow().as_slice(),
                                                        *buffered_event_limit,
                                                    );
                                                    message_events.set(snapshot);
                                                    pending_render_for_callback.borrow_mut().take();
                                                }),
                                            );
                                        }
                                    }
                                    Err(parse_error) => {
                                        message_error.emit(parse_error.to_string());
                                    }
                                }
                            }
                        })
                            as Box<dyn FnMut(_)>);
                        ws.set_onmessage(Some(on_message.as_ref().unchecked_ref()));

                        let err_cb = on_error_cb.clone();
                        let on_error = Closure::wrap(Box::new(move |_event: ErrorEvent| {
                            err_cb.emit("websocket stream error".to_owned());
                        })
                            as Box<dyn FnMut(_)>);
                        ws.set_onerror(Some(on_error.as_ref().unchecked_ref()));

                        let set_close = {
                            let internal_connection = internal_connection.clone();
                            let on_connection_change = on_connection_change.clone();
                            move || {
                                internal_connection.set("disconnected".to_owned());
                                on_connection_change.emit("disconnected".to_owned());
                            }
                        };
                        let on_close = Closure::wrap(Box::new(move |_event: Event| {
                            set_close();
                        })
                            as Box<dyn FnMut(_)>);
                        ws.set_onclose(Some(on_close.as_ref().unchecked_ref()));

                        *socket.borrow_mut() = Some(WsBindings {
                            socket: ws,
                            _on_open: on_open,
                            _on_message: on_message,
                            _on_error: on_error,
                            _on_close: on_close,
                        });
                    }
                    Err(_) => {
                        set_connection("disconnected");
                        on_error_cb.emit("failed to open websocket".to_owned());
                    }
                }
            } else {
                set_connection("disconnected");
            }

            let socket = socket.clone();
            let pending_render = pending_render.clone();
            move || {
                pending_render.borrow_mut().take();
                if let Some(existing) = socket.borrow_mut().take() {
                    let _ = existing.socket.close();
                }
            }
        });
    }

    // Buffer limit change
    {
        let events = events.clone();
        let current_page = current_page.clone();
        let event_store = event_store.clone();
        use_effect_with(*buffered_event_limit, move |buffered_event_limit| {
            let next =
                visible_buffered_events(event_store.borrow().as_slice(), *buffered_event_limit);
            events.set(next);
            current_page.set(0);
            || {}
        });
    }

    // Derived state
    let status_class = connection_status_class(&connection);
    let search_query_value = search_query.trim().to_ascii_lowercase();

    let matched_events = use_memo(
        (events.clone(), search_query_value.clone()),
        |(events, query)| {
            events
                .iter()
                .filter(|e| matches_search_query(e, query))
                .cloned()
                .collect::<Vec<_>>()
        },
    );
    let matched_events = &*matched_events;
    let total_events = matched_events.len();
    let total_pages = total_events.max(1).div_ceil(EVENT_PAGE_SIZE);
    let page = (*current_page).min(total_pages.saturating_sub(1));
    let end_index = total_events.saturating_sub(page * EVENT_PAGE_SIZE);
    let start_index = end_index.saturating_sub(EVENT_PAGE_SIZE);
    let shown_events = end_index.saturating_sub(start_index);
    let visible_events = matched_events[start_index..end_index].to_vec();

    let filtered_stream_events = use_memo(
        (
            events.clone(),
            (*stream_filter).clone(),
            search_query_value.clone(),
        ),
        |(events, filter, query)| {
            events
                .iter()
                .filter(|e| matches_stream_filter(e, filter) && matches_search_query(e, query))
                .cloned()
                .collect::<Vec<_>>()
        },
    );
    let filtered_stream_events = &*filtered_stream_events;

    let available_sources = use_memo(events.clone(), |events| available_device_sources(events));
    let available_sources = &*available_sources;

    let show_serial_write_panel = *view_mode == LogViewMode::Stream
        && matches!(&*stream_filter, StreamFilter::Source(LogSource::Serial))
        && props.serial_write_device_id.is_some();

    // Callbacks
    let on_search_input = {
        let search_query = search_query.clone();
        let current_page = current_page.clone();
        Callback::from(move |event: InputEvent| {
            let input: HtmlInputElement = event.target_unchecked_into();
            search_query.set(input.value());
            current_page.set(0);
        })
    };

    let on_buffer_limit_change = {
        let buffered_event_limit = buffered_event_limit.clone();
        Callback::from(move |event: InputEvent| {
            let select: HtmlSelectElement = event.target_unchecked_into();
            if let Ok(v) = select.value().parse::<usize>() {
                buffered_event_limit.set(v);
            }
        })
    };

    let on_show_older = {
        let current_page = current_page.clone();
        Callback::from(move |_| current_page.set(*current_page + 1))
    };
    let on_show_newer = {
        let current_page = current_page.clone();
        Callback::from(move |_| current_page.set(current_page.saturating_sub(1)))
    };
    let on_show_event_view = {
        let view_mode = view_mode.clone();
        Callback::from(move |_| view_mode.set(LogViewMode::Event))
    };
    let on_show_stream_view = {
        let view_mode = view_mode.clone();
        Callback::from(move |_| view_mode.set(LogViewMode::Stream))
    };

    let on_filter_all = {
        let f = stream_filter.clone();
        Callback::from(move |_| f.set(StreamFilter::All))
    };
    let on_filter_device_logs = {
        let f = stream_filter.clone();
        Callback::from(move |_| f.set(StreamFilter::DeviceLogs))
    };
    let on_filter_structured = {
        let f = stream_filter.clone();
        Callback::from(move |_| f.set(StreamFilter::Structured))
    };

    let on_jump_to_latest = {
        let scroll_to_bottom_revision = scroll_to_bottom_revision.clone();
        Callback::from(move |_| {
            scroll_to_bottom_revision.set(scroll_to_bottom_revision.wrapping_add(1));
        })
    };

    let on_clear_cache = {
        let events = events.clone();
        let event_store = event_store.clone();
        let dedup_set = dedup_set.clone();
        let current_page = current_page.clone();
        let pending_render = pending_render.clone();
        let clear_cache_status = clear_cache_status.clone();
        let clear_cache_pending = clear_cache_pending.clone();
        Callback::from(move |_| {
            if *clear_cache_pending {
                return;
            }
            clear_cache_pending.set(true);
            let events = events.clone();
            let event_store = event_store.clone();
            let dedup_set = dedup_set.clone();
            let current_page = current_page.clone();
            let pending_render = pending_render.clone();
            let clear_cache_status = clear_cache_status.clone();
            let clear_cache_pending = clear_cache_pending.clone();
            spawn_local(async move {
                match api::clear_event_cache().await {
                    Ok(result) => {
                        pending_render.borrow_mut().take();
                        event_store.borrow_mut().clear();
                        dedup_set.borrow_mut().clear();
                        events.set(Vec::new());
                        current_page.set(0);
                        clear_cache_status.set(Some(format!(
                            "Cleared {} cached events",
                            result.cleared_events
                        )));
                    }
                    Err(message) => {
                        clear_cache_status.set(Some(format!("Clear failed: {message}")));
                    }
                }
                clear_cache_pending.set(false);
            });
        })
    };

    let on_bottom_state_change = {
        let stream_at_bottom = stream_at_bottom.clone();
        Callback::from(move |at_bottom: bool| stream_at_bottom.set(at_bottom))
    };

    html! {
        <article class="dh-panel rounded-[24px] p-6">
            <div class="mb-4 grid gap-4 lg:grid-cols-[minmax(0,1fr)_14rem]">
                <label class="grid gap-2 text-sm text-ink-muted">
                    <span>{"Search logs"}</span>
                    <input
                        class="w-full rounded-2xl dh-input"
                        value={(*search_query).clone()}
                        oninput={on_search_input}
                        placeholder="message, source, level, device_id, job_id"
                    />
                </label>
                <label class="grid gap-2 text-sm text-ink-muted">
                    <span>{"Buffered cap"}</span>
                    {
                        {
                            let cap = *buffered_event_limit;
                            html! {
                                <select
                                    class="w-full rounded-2xl dh-input"
                                    oninput={on_buffer_limit_change}
                                >
                                    {
                                        for BUFFERED_EVENT_LIMIT_OPTIONS.iter().map(|option| {
                                            html! {
                                                <option value={option.to_string()} selected={cap == *option}>
                                                    {option.to_string()}
                                                </option>
                                            }
                                        })
                                    }
                                </select>
                            }
                        }
                    }
                </label>
            </div>
            <div class="flex flex-wrap items-center justify-between gap-3">
                <div class="flex flex-wrap items-center gap-3">
                    <span class="inline-flex items-center gap-2 uppercase tracking-[0.18em] text-[0.74rem] text-accent-strong">{"Logs"}</span>
                    <div class="inline-flex rounded-full dh-surface p-1">
                        <button
                            class={toggle_button_class(*view_mode == LogViewMode::Stream)}
                            onclick={on_show_stream_view}
                        >{"Stream"}</button>
                        <button
                            class={toggle_button_class(*view_mode == LogViewMode::Event)}
                            onclick={on_show_event_view}
                        >{"Event"}</button>
                    </div>
                    <span class={status_class}>{connection.clone()}</span>
                </div>
                <span class="font-mono text-sm text-ink-muted">{
                    format!("{} matched / {} buffered · page {} / {} · showing {} of {}",
                        matched_events.len(), events.len(),
                        page + 1, total_pages, shown_events, total_events)
                }</span>
            </div>
            <div class="mb-5 mt-4 flex flex-wrap items-center justify-between gap-3">
                <div class="flex flex-wrap items-center gap-2.5">
                    {
                        if *view_mode == LogViewMode::Stream {
                            html! {
                                <div class="inline-flex rounded-full dh-surface p-1">
                                    <button class={filter_button_class(*stream_filter == StreamFilter::All)} onclick={on_filter_all}>{"All"}</button>
                                    <button class={filter_button_class(*stream_filter == StreamFilter::DeviceLogs)} onclick={on_filter_device_logs}>{"Device logs"}</button>
                                    {
                                        for available_sources.iter().cloned().map(|source| {
                                            let sf = stream_filter.clone();
                                            let active = *sf == StreamFilter::Source(source.clone());
                                            let label = format_source_label(&source);
                                            let src = source.clone();
                                            let onclick = Callback::from(move |_| sf.set(StreamFilter::Source(src.clone())));
                                            html! { <button class={filter_button_class(active)} {onclick}>{label}</button> }
                                        })
                                    }
                                    <button class={filter_button_class(*stream_filter == StreamFilter::Structured)} onclick={on_filter_structured}>{"Structured"}</button>
                                </div>
                            }
                        } else {
                            Html::default()
                        }
                    }
                </div>
                <div class="flex flex-wrap items-center gap-2.5">
                    {
                        if props.show_clear_cache {
                            html! {
                                <button
                                    class="rounded-2xl border border-red-200 bg-red-50 px-4 py-3 font-medium text-red-800 transition hover:bg-red-100 disabled:cursor-default disabled:opacity-55"
                                    onclick={on_clear_cache}
                                    disabled={*clear_cache_pending}
                                >{if *clear_cache_pending { "Clearing..." } else { "Clear cache" }}</button>
                            }
                        } else {
                            Html::default()
                        }
                    }
                    {
                        if *view_mode == LogViewMode::Event {
                            html! {
                                <>
                                    <button class="rounded-2xl dh-button px-4 py-3" onclick={on_show_newer} disabled={page == 0}>{"Newer"}</button>
                                    <button class="rounded-2xl dh-button px-4 py-3" onclick={on_show_older} disabled={start_index == 0 || total_events == 0}>{"Older"}</button>
                                </>
                            }
                        } else {
                            html! {
                                <>
                                    <span class="font-mono text-sm text-ink-muted">{format!("{} streamed events", filtered_stream_events.len())}</span>
                                    if !*stream_at_bottom {
                                        <button
                                            class="rounded-2xl bg-gradient-to-br from-accent to-accent-strong px-4 py-3 font-medium text-white transition hover:opacity-95"
                                            onclick={on_jump_to_latest}
                                        >{"Jump to latest"}</button>
                                    }
                                </>
                            }
                        }
                    }
                </div>
            </div>
            {
                if let Some(message) = (*clear_cache_status).clone() {
                    html! { <p class="mb-4 mt-0 text-sm text-ink-muted">{message}</p> }
                } else {
                    Html::default()
                }
            }
            <div class="min-w-0 grid gap-4">
                {
                    if events.is_empty() {
                        html! { <p class="m-0 text-ink-muted">{"Waiting for events..."}</p> }
                    } else if *view_mode == LogViewMode::Event {
                        html! {
                            for visible_events
                                .iter()
                                .rev()
                                .cloned()
                                .map(|event| html! { <LogEventCard event={event} /> })
                        }
                    } else if filtered_stream_events.is_empty() {
                        html! { <p class="m-0 text-ink-muted">{"No logs match the current stream filter."}</p> }
                    } else {
                        html! {
                            {view_stream_terminal(
                                filtered_stream_events.as_slice(),
                                format!(
                                    "{}|{}",
                                    stream_filter_key(&stream_filter),
                                    search_query_value
                                ),
                                *scroll_to_bottom_revision,
                                on_bottom_state_change.clone(),
                            )}
                        }
                    }
                }
            </div>
            {
                if show_serial_write_panel {
                    html! {
                        <SerialWritePanel device_id={props.serial_write_device_id.clone().unwrap_or_default()} />
                    }
                } else {
                    Html::default()
                }
            }
        </article>
    }
}

// ---------------------------------------------------------------------------
// Identity hash for O(1) dedup
// ---------------------------------------------------------------------------

fn event_identity_hash(event: &LogEvent) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    event.ts.hash(&mut hasher);
    event.device_id.hash(&mut hasher);
    event.job_id.hash(&mut hasher);
    event.message.hash(&mut hasher);
    std::mem::discriminant(&event.source).hash(&mut hasher);
    std::mem::discriminant(&event.channel).hash(&mut hasher);
    std::mem::discriminant(&event.level).hash(&mut hasher);
    hasher.finish()
}

// ---------------------------------------------------------------------------
// Pure helper functions
// ---------------------------------------------------------------------------

pub(crate) fn matches_search_query(event: &LogEvent, query: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    let source = format_source_label(&event.source);
    let channel = match event.channel {
        LogChannel::Stdout => "stdout",
        LogChannel::Stderr => "stderr",
        LogChannel::System => "system",
    };
    let level = match event.level {
        LogLevel::Debug => "debug",
        LogLevel::Info => "info",
        LogLevel::Warn => "warn",
        LogLevel::Error => "error",
    };
    [
        event.message.as_str(),
        event.device_id.as_str(),
        event.job_id.as_str(),
        source,
        channel,
        level,
    ]
    .into_iter()
    .any(|v| v.to_ascii_lowercase().contains(query))
}

pub(crate) fn trim_buffered_events(events: &mut Vec<LogEvent>, max_events: usize) {
    if events.len() > max_events {
        let drain = events.len() - max_events;
        events.drain(0..drain);
    }
}

fn visible_buffered_events(events: &[LogEvent], max_events: usize) -> Vec<LogEvent> {
    let start = events.len().saturating_sub(max_events);
    events[start..].to_vec()
}

fn matches_stream_filter(event: &LogEvent, filter: &StreamFilter) -> bool {
    let is_structured = matches!(event.source, LogSource::System | LogSource::Job)
        || matches!(event.channel, LogChannel::System);
    match filter {
        StreamFilter::All => true,
        StreamFilter::DeviceLogs => !is_structured,
        StreamFilter::Structured => is_structured,
        StreamFilter::Source(source) => event.source == *source && !is_structured,
    }
}

fn available_device_sources(events: &[LogEvent]) -> Vec<LogSource> {
    [
        LogSource::Serial,
        LogSource::Ssh,
        LogSource::Jlink,
        LogSource::Adb,
    ]
    .into_iter()
    .filter(|source| events.iter().any(|e| e.source == *source))
    .collect()
}

pub(crate) fn format_source_label(source: &LogSource) -> &'static str {
    match source {
        LogSource::Serial => "serial",
        LogSource::Ssh => "ssh",
        LogSource::Jlink => "jlink",
        LogSource::Adb => "adb",
        LogSource::System => "system",
        LogSource::Job => "job",
    }
}

fn stream_filter_key(filter: &StreamFilter) -> String {
    match filter {
        StreamFilter::All => "all".into(),
        StreamFilter::DeviceLogs => "device-logs".into(),
        StreamFilter::Structured => "structured".into(),
        StreamFilter::Source(source) => format!("source:{}", format_source_label(source)),
    }
}

// ---------------------------------------------------------------------------
// Stream terminal view
// ---------------------------------------------------------------------------

fn view_stream_terminal(
    stream_events: &[LogEvent],
    reset_key: String,
    scroll_to_bottom_revision: u32,
    on_bottom_state_change: Callback<bool>,
) -> Html {
    html! {
        <div class="overflow-hidden rounded-[20px] dh-surface shadow-inner">
            <div class="flex flex-wrap items-center justify-between gap-3 border-b border-white/10 bg-white/5 px-4 py-3">
                <div class="flex flex-wrap items-center gap-2">
                    <span class="inline-flex rounded-full bg-white/10 px-3 py-1.5 text-xs font-medium text-slate-100">{"stream"}</span>
                    <span class="inline-flex rounded-full bg-white/10 px-3 py-1.5 text-xs font-medium text-slate-300">{format!("{} events", stream_events.len())}</span>
                </div>
            </div>
            <XtermTerminal
                events={stream_events.to_vec()}
                {reset_key}
                {scroll_to_bottom_revision}
                {on_bottom_state_change}
            />
        </div>
    }
}

fn append_stream_ws_query(
    base_ws_url: Option<String>,
    stream_filter: &StreamFilter,
    search_query: &str,
) -> Option<String> {
    let mut url = base_ws_url?;
    let mut params = Vec::new();

    match stream_filter {
        StreamFilter::All => {}
        StreamFilter::DeviceLogs => params.push("structured_only=false".to_owned()),
        StreamFilter::Structured => params.push("structured_only=true".to_owned()),
        StreamFilter::Source(source) => {
            params.push(format!(
                "source={}",
                urlencoding::encode(format_source_label(source))
            ));
            params.push("structured_only=false".to_owned());
        }
    }

    let trimmed_search = search_query.trim();
    if !trimmed_search.is_empty() {
        params.push(format!("search={}", urlencoding::encode(trimmed_search)));
    }

    if params.is_empty() {
        return Some(url);
    }

    let joiner = if url.contains('?') { '&' } else { '?' };
    url.push(joiner);
    url.push_str(&params.join("&"));
    Some(url)
}

// ---------------------------------------------------------------------------
// CSS class helpers
// ---------------------------------------------------------------------------

fn connection_status_class(status: &str) -> &'static str {
    match status {
        "connected" => {
            "inline-flex rounded-full bg-accent/10 px-3 py-1.5 text-xs font-medium text-accent-strong"
        }
        "connecting" => {
            "inline-flex rounded-full bg-amber-500/15 px-3 py-1.5 text-xs font-medium text-amber-800"
        }
        _ => "inline-flex rounded-full dh-pill px-3 py-1.5 text-xs font-medium text-ink-muted",
    }
}

fn toggle_button_class(active: bool) -> Classes {
    classes!(
        "rounded-full",
        "px-3",
        "py-1.5",
        "text-sm",
        "font-medium",
        "transition",
        if active {
            "bg-accent text-white"
        } else {
            "text-ink-muted hover:text-ink"
        }
    )
}

fn filter_button_class(active: bool) -> Classes {
    toggle_button_class(active)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn sample_event(index: usize, message: impl Into<String>) -> LogEvent {
        LogEvent {
            ts: chrono::Utc
                .timestamp_opt(index as i64, 0)
                .single()
                .expect("valid timestamp"),
            device_id: "board-alpha".into(),
            job_id: "job-1".into(),
            source: LogSource::Serial,
            channel: LogChannel::Stdout,
            level: LogLevel::Info,
            message: message.into(),
        }
    }

    #[test]
    fn trims_large_event_buffers_without_dropping_latest_logs() {
        let mut events = (0..6)
            .map(|i| sample_event(i, format!("line {i}")))
            .collect::<Vec<_>>();
        trim_buffered_events(&mut events, 3);
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].message, "line 3");
        assert_eq!(events[2].message, "line 5");
    }

    #[test]
    fn visible_buffered_events_preserve_retained_history_when_cap_changes() {
        let mut retained = (0..6)
            .map(|i| sample_event(i, format!("line {i}")))
            .collect::<Vec<_>>();
        trim_buffered_events(&mut retained, 5);
        let visible = visible_buffered_events(&retained, 3);
        assert_eq!(retained.len(), 5);
        assert_eq!(retained[0].message, "line 1");
        assert_eq!(visible.len(), 3);
        assert_eq!(visible[0].message, "line 3");
        assert_eq!(visible[2].message, "line 5");
    }

    #[test]
    fn append_stream_ws_query_adds_source_and_search_filters() {
        let url = append_stream_ws_query(
            Some("ws://localhost/api/ws?device_id=board-alpha".into()),
            &StreamFilter::Source(LogSource::Serial),
            "boot failed",
        )
        .expect("ws url");

        assert!(url.contains("device_id=board-alpha"));
        assert!(url.contains("source=serial"));
        assert!(url.contains("structured_only=false"));
        assert!(url.contains("search=boot%20failed"));
    }

    #[test]
    fn append_stream_ws_query_omits_extra_filters_for_all_streams() {
        let url =
            append_stream_ws_query(Some("ws://localhost/api/ws".into()), &StreamFilter::All, "")
                .expect("ws url");

        assert_eq!(url, "ws://localhost/api/ws");
    }

    #[test]
    fn event_identity_hash_is_stable_for_same_event() {
        let a = sample_event(0, "hello");
        let b = sample_event(0, "hello");
        assert_eq!(event_identity_hash(&a), event_identity_hash(&b));
    }

    #[test]
    fn event_identity_hash_differs_for_different_messages() {
        let a = sample_event(0, "hello");
        let b = sample_event(0, "world");
        assert_ne!(event_identity_hash(&a), event_identity_hash(&b));
    }
}
