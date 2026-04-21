use std::collections::HashSet;

use chrono::{DateTime, Utc};
use device_hub_shared::{LogChannel, LogEvent, LogLevel, LogSource};
use gloo_timers::callback::Timeout;
use wasm_bindgen::{JsCast, closure::Closure};
use web_sys::{
    ErrorEvent, Event, HtmlElement, HtmlInputElement, HtmlSelectElement, KeyboardEvent,
    MessageEvent, MouseEvent, WebSocket, window,
};
use yew::prelude::*;

use crate::components::{
    log_event::{LogEventCard, plain_log_text, render_log_message, visible_log_text},
    serial_write_panel::SerialWritePanel,
};

const EVENT_PAGE_SIZE: usize = 50;
const DEFAULT_BUFFERED_EVENTS: usize = 5_000;
const MAX_BUFFERED_EVENTS: usize = 5_000;
const RENDER_FLUSH_DELAY_MS: u32 = 32;
// Rendering a few hundred chunks is still cheap enough here and avoids the
// visible blank gap that can appear when a fast scroll outruns the virtualized
// window for a frame.
const STREAM_VIRTUALIZATION_MIN_CHUNKS: usize = 600;
const STREAM_VIRTUALIZATION_OVERSCAN_PX: i32 = 1_600;
const STREAM_CHUNK_BASE_HEIGHT_PX: i32 = 56;
const STREAM_CHUNK_LINE_HEIGHT_PX: i32 = 20;
const STREAM_WRAP_WIDTH_CHARS: usize = 80;
const STREAM_CHUNK_PLAIN_RENDER_LINE_THRESHOLD: usize = 160;
const STREAM_CHUNK_PLAIN_RENDER_CHAR_THRESHOLD: usize = 32_000;
const STREAM_TAIL_LOCK_THRESHOLD_PX: i32 = 64;

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

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct StreamChunk {
    device_id: String,
    source: LogSource,
    channel: LogChannel,
    level: LogLevel,
    started_at: DateTime<Utc>,
    ended_at: DateTime<Utc>,
    lines: Vec<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct StreamWindow {
    start_index: usize,
    end_index: usize,
    top_padding_px: i32,
    bottom_padding_px: i32,
}

#[derive(Clone, PartialEq, Eq)]
struct StreamAnchor {
    device_id: String,
    source: LogSource,
    channel: LogChannel,
    started_at: DateTime<Utc>,
    offset_px: i32,
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
    let internal_connection = use_state(|| "connecting".to_owned());
    let view_mode = use_state(|| LogViewMode::Stream);
    let stream_filter = use_state(|| StreamFilter::DeviceLogs);
    let follow_stream_tail = use_state(|| true);
    let logs_fullscreen = use_state(|| false);
    let stream_scroll_top = use_state(|| 0_i32);
    let stream_viewport_height = use_state(|| 720_i32);
    let selection_lock_active = use_state(|| false);
    let frozen_stream_chunks = use_state(|| Option::<Vec<StreamChunk>>::None);
    let stream_container_ref = use_node_ref();
    let last_stream_scroll_top = use_mut_ref(|| 0_i32);
    let stream_anchor = use_mut_ref(|| Option::<StreamAnchor>::None);
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
        let ws_url = props.ws_url.clone();
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

    let stream_chunks = use_memo(filtered_stream_events.clone(), |events| {
        build_stream_chunks(events)
    });
    let stream_chunks = &*stream_chunks;

    let displayed_stream_chunks = if *selection_lock_active {
        (*frozen_stream_chunks)
            .clone()
            .unwrap_or_else(|| (*stream_chunks).clone())
    } else {
        (*stream_chunks).clone()
    };

    let stream_window = compute_stream_window(
        &displayed_stream_chunks,
        *stream_scroll_top,
        *stream_viewport_height,
        STREAM_VIRTUALIZATION_OVERSCAN_PX,
    );
    let visible_stream_chunks =
        &displayed_stream_chunks[stream_window.start_index..stream_window.end_index];

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

    let on_jump_to_latest = {
        let follow_stream_tail = follow_stream_tail.clone();
        let stream_scroll_top = stream_scroll_top.clone();
        let selection_lock_active = selection_lock_active.clone();
        let stream_container_ref = stream_container_ref.clone();
        let last_stream_scroll_top = last_stream_scroll_top.clone();
        let stream_anchor = stream_anchor.clone();
        Callback::from(move |_| {
            selection_lock_active.set(false);
            follow_stream_tail.set(true);
            *stream_anchor.borrow_mut() = None;
            if let Some(el) = stream_container_ref.cast::<HtmlElement>() {
                let scroll_top = scroll_stream_to_bottom(&el);
                *last_stream_scroll_top.borrow_mut() = scroll_top;
                stream_scroll_top.set(scroll_top);
                if let Some(w) = window() {
                    let stream_scroll_top = stream_scroll_top.clone();
                    let last_stream_scroll_top = last_stream_scroll_top.clone();
                    let cb = Closure::once_into_js(move || {
                        let scroll_top = scroll_stream_to_bottom(&el);
                        *last_stream_scroll_top.borrow_mut() = scroll_top;
                        stream_scroll_top.set(scroll_top);
                    });
                    let _ = w.request_animation_frame(cb.as_ref().unchecked_ref());
                }
            }
        })
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

    let pending_scroll_raf = use_mut_ref(|| Option::<i32>::None);
    let on_stream_scroll = {
        let stream_container_ref = stream_container_ref.clone();
        let follow_stream_tail = follow_stream_tail.clone();
        let stream_scroll_top = stream_scroll_top.clone();
        let stream_viewport_height = stream_viewport_height.clone();
        let last_stream_scroll_top = last_stream_scroll_top.clone();
        let displayed_stream_chunks = displayed_stream_chunks.clone();
        let stream_anchor = stream_anchor.clone();
        let pending_scroll_raf = pending_scroll_raf.clone();
        Callback::from(move |_| {
            if let Some(el) = stream_container_ref.cast::<HtmlElement>() {
                let st = el.scroll_top();
                *last_stream_scroll_top.borrow_mut() = st;
                let at_tail = is_stream_scrolled_to_bottom(&el, STREAM_TAIL_LOCK_THRESHOLD_PX);
                if at_tail {
                    *stream_anchor.borrow_mut() = None;
                } else {
                    *stream_anchor.borrow_mut() =
                        capture_stream_anchor(&displayed_stream_chunks, st);
                }
                follow_stream_tail.set(at_tail);
            }
            if pending_scroll_raf.borrow().is_some() {
                return;
            }
            let stream_container_ref = stream_container_ref.clone();
            let follow_stream_tail = follow_stream_tail.clone();
            let stream_scroll_top = stream_scroll_top.clone();
            let stream_viewport_height = stream_viewport_height.clone();
            let last_stream_scroll_top = last_stream_scroll_top.clone();
            let displayed_stream_chunks = displayed_stream_chunks.clone();
            let stream_anchor = stream_anchor.clone();
            let raf_pending = pending_scroll_raf.clone();
            let cb = Closure::once_into_js(move || {
                *raf_pending.borrow_mut() = None;
                if let Some(el) = stream_container_ref.cast::<HtmlElement>() {
                    let st = el.scroll_top();
                    *last_stream_scroll_top.borrow_mut() = st;
                    stream_scroll_top.set(st);
                    stream_viewport_height.set(el.client_height());
                    let at_tail = is_stream_scrolled_to_bottom(&el, STREAM_TAIL_LOCK_THRESHOLD_PX);
                    if at_tail {
                        *stream_anchor.borrow_mut() = None;
                    } else {
                        *stream_anchor.borrow_mut() =
                            capture_stream_anchor(&displayed_stream_chunks, st);
                    }
                    follow_stream_tail.set(at_tail);
                }
            });
            if let Some(w) = window() {
                if let Ok(id) = w.request_animation_frame(cb.as_ref().unchecked_ref()) {
                    *pending_scroll_raf.borrow_mut() = Some(id);
                }
            }
        })
    };

    // Freeze stream chunks while text selection is active
    {
        let frozen = frozen_stream_chunks.clone();
        let chunks = (*stream_chunks).clone();
        let locked = selection_lock_active.clone();
        use_effect_with((*locked, chunks.clone()), move |_| {
            if *locked {
                if frozen.is_none() {
                    frozen.set(Some(chunks.clone()));
                }
            } else if frozen.is_some() {
                frozen.set(None);
            }
            || {}
        });
    }

    // Auto-scroll / anchor restore
    {
        let stream_container_ref = stream_container_ref.clone();
        let follow_stream_tail = follow_stream_tail.clone();
        let stream_scroll_top = stream_scroll_top.clone();
        let stream_viewport_height = stream_viewport_height.clone();
        let selection_lock_active = selection_lock_active.clone();
        let last_stream_scroll_top = last_stream_scroll_top.clone();
        let stream_anchor = stream_anchor.clone();
        let view_mode_value = *view_mode;
        let chunk_count = displayed_stream_chunks.len();
        let event_count = filtered_stream_events.len();
        let chunks_for_restore = displayed_stream_chunks.clone();
        use_effect_with(
            (
                view_mode_value,
                chunk_count,
                event_count,
                *follow_stream_tail,
                *selection_lock_active,
            ),
            move |_| {
                if view_mode_value == LogViewMode::Stream {
                    if let Some(el) = stream_container_ref.cast::<HtmlElement>() {
                        stream_viewport_height.set(el.client_height());
                        if *selection_lock_active {
                            *last_stream_scroll_top.borrow_mut() = el.scroll_top();
                            stream_scroll_top.set(el.scroll_top());
                        } else if *follow_stream_tail {
                            *stream_anchor.borrow_mut() = None;
                            let scroll_top = scroll_stream_to_bottom(&el);
                            *last_stream_scroll_top.borrow_mut() = scroll_top;
                            stream_scroll_top.set(scroll_top);
                            if let Some(w) = window() {
                                let stream_scroll_top = stream_scroll_top.clone();
                                let last_stream_scroll_top = last_stream_scroll_top.clone();
                                let cb = Closure::once_into_js(move || {
                                    let scroll_top = scroll_stream_to_bottom(&el);
                                    *last_stream_scroll_top.borrow_mut() = scroll_top;
                                    stream_scroll_top.set(scroll_top);
                                });
                                let _ = w.request_animation_frame(cb.as_ref().unchecked_ref());
                            }
                        } else {
                            let restored = stream_anchor
                                .borrow()
                                .as_ref()
                                .and_then(|a| {
                                    restore_stream_anchor_scroll_top(&chunks_for_restore, a)
                                })
                                .unwrap_or(*last_stream_scroll_top.borrow());
                            el.set_scroll_top(restored);
                            stream_scroll_top.set(el.scroll_top());
                            *last_stream_scroll_top.borrow_mut() = el.scroll_top();
                        }
                    }
                }
                || {}
            },
        );
    }

    // Reset anchor on view/filter change
    {
        let last_stream_scroll_top = last_stream_scroll_top.clone();
        let stream_anchor = stream_anchor.clone();
        let vm = *view_mode;
        let sf = (*stream_filter).clone();
        use_effect_with((vm, sf), move |_| {
            *last_stream_scroll_top.borrow_mut() = 0;
            *stream_anchor.borrow_mut() = None;
            || {}
        });
    }

    let on_stream_select_start = {
        let locked = selection_lock_active.clone();
        Callback::from(move |_: Event| locked.set(true))
    };
    let on_stream_selection_change = {
        let locked = selection_lock_active.clone();
        Callback::from(move |_: MouseEvent| locked.set(has_active_text_selection()))
    };
    let on_stream_selection_key_up = {
        let locked = selection_lock_active.clone();
        Callback::from(move |_: KeyboardEvent| locked.set(has_active_text_selection()))
    };

    let on_toggle_logs_fullscreen = {
        let fs = logs_fullscreen.clone();
        Callback::from(move |_| fs.set(!*fs))
    };

    {
        let fs = logs_fullscreen.clone();
        use_effect_with(*fs, move |is_fs| {
            set_page_scroll_locked(*is_fs);
            || set_page_scroll_locked(false)
        });
    }

    html! {
        <article class={classes!(
            "rounded-[24px]",
            "border",
            "border-black/10",
            "bg-white/80",
            "p-6",
            "shadow-shell",
            "backdrop-blur",
            if *logs_fullscreen {
                Some("fixed inset-4 z-[70] overflow-auto bg-white/95")
            } else {
                None
            }
        )}>
            <div class="mb-4 grid gap-4 lg:grid-cols-[minmax(0,1fr)_14rem]">
                <label class="grid gap-2 text-sm text-ink-muted">
                    <span>{"Search logs"}</span>
                    <input
                        class="w-full rounded-2xl border border-black/10 bg-white/75 px-4 py-3 text-ink outline-none transition placeholder:text-ink-muted/70 focus:border-accent/40 focus:ring-2 focus:ring-accent/10"
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
                                    class="w-full rounded-2xl border border-black/10 bg-white/75 px-4 py-3 text-ink outline-none transition focus:border-accent/40 focus:ring-2 focus:ring-accent/10"
                                    oninput={on_buffer_limit_change}
                                >
                                    <option value="500" selected={cap == 500}>{"500"}</option>
                                    <option value="1000" selected={cap == 1000}>{"1000"}</option>
                                    <option value="2000" selected={cap == 2000}>{"2000"}</option>
                                    <option value="5000" selected={cap == 5000}>{"5000"}</option>
                                </select>
                            }
                        }
                    }
                </label>
            </div>
            <div class="flex flex-wrap items-center justify-between gap-3">
                <div class="flex flex-wrap items-center gap-3">
                    <span class="inline-flex items-center gap-2 uppercase tracking-[0.18em] text-[0.74rem] text-accent-strong">{"Logs"}</span>
                    <div class="inline-flex rounded-full border border-black/10 bg-white/75 p-1">
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
                                <div class="inline-flex rounded-full border border-black/10 bg-white/75 p-1">
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
                        if *view_mode == LogViewMode::Event {
                            html! {
                                <>
                                    <button class="rounded-2xl border border-black/10 bg-white/75 px-4 py-3 font-medium text-ink transition hover:bg-white disabled:cursor-default disabled:opacity-55" onclick={on_show_newer} disabled={page == 0}>{"Newer"}</button>
                                    <button class="rounded-2xl border border-black/10 bg-white/75 px-4 py-3 font-medium text-ink transition hover:bg-white disabled:cursor-default disabled:opacity-55" onclick={on_show_older} disabled={start_index == 0 || total_events == 0}>{"Older"}</button>
                                </>
                            }
                        } else {
                            html! {
                                <>
                                    <span class="font-mono text-sm text-ink-muted">{format!("{} events -> {} chunks", filtered_stream_events.len(), displayed_stream_chunks.len())}</span>
                                    <button
                                        class="rounded-2xl border border-black/10 bg-white/75 px-4 py-3 font-medium text-ink transition hover:bg-white disabled:cursor-default disabled:opacity-55"
                                        onclick={on_jump_to_latest}
                                        disabled={*follow_stream_tail && !*selection_lock_active}
                                    >{"Jump to latest"}</button>
                                </>
                            }
                        }
                    }
                    {
                        if *view_mode == LogViewMode::Stream && *selection_lock_active {
                            html! { <span class="font-mono text-sm text-amber-700">{"selection locked"}</span> }
                        } else if *view_mode == LogViewMode::Stream && !*follow_stream_tail {
                            html! { <span class="font-mono text-sm text-amber-700">{"reading locked"}</span> }
                        } else {
                            Html::default()
                        }
                    }
                    <button
                        class="rounded-2xl border border-black/10 bg-white/75 px-4 py-3 font-medium text-ink transition hover:bg-white"
                        onclick={on_toggle_logs_fullscreen.clone()}
                    >{if *logs_fullscreen { "Exit full screen" } else { "Full screen" }}</button>
                </div>
            </div>
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
                    } else if stream_chunks.is_empty() {
                        html! { <p class="m-0 text-ink-muted">{"No logs match the current stream filter."}</p> }
                    } else {
                        html! {
                            {view_stream_terminal(
                                &displayed_stream_chunks,
                                visible_stream_chunks,
                                stream_window,
                                stream_container_ref.clone(),
                                on_stream_scroll.clone(),
                                on_stream_select_start.clone(),
                                on_stream_selection_change.clone(),
                                on_stream_selection_key_up.clone(),
                                *selection_lock_active,
                                *logs_fullscreen,
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

fn is_stream_scrolled_to_bottom(element: &HtmlElement, threshold_px: i32) -> bool {
    let distance = element.scroll_height() - element.scroll_top() - element.client_height();
    distance <= threshold_px.max(0)
}

fn scroll_stream_to_bottom(element: &HtmlElement) -> i32 {
    element.set_scroll_top(element.scroll_height());
    element.scroll_top()
}

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

pub(crate) fn build_stream_chunks(events: &[LogEvent]) -> Vec<StreamChunk> {
    let mut chunks: Vec<StreamChunk> = Vec::new();
    for event in events {
        let normalized = event.message.trim_end_matches('\n').to_owned();
        if let Some(last) = chunks.last_mut() {
            if last.device_id == event.device_id
                && last.source == event.source
                && last.channel == event.channel
                && should_merge_stream_chunk(last, event)
            {
                last.ended_at = event.ts;
                last.level = merge_level(&last.level, &event.level);
                last.lines.push(normalized);
                continue;
            }
        }
        chunks.push(StreamChunk {
            device_id: event.device_id.clone(),
            source: event.source.clone(),
            channel: event.channel.clone(),
            level: event.level.clone(),
            started_at: event.ts,
            ended_at: event.ts,
            lines: vec![normalized],
        });
    }
    chunks
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

fn should_merge_stream_chunk(chunk: &StreamChunk, event: &LogEvent) -> bool {
    let is_structured = matches!(event.source, LogSource::System | LogSource::Job)
        || matches!(event.channel, LogChannel::System);
    if is_structured {
        return false;
    }
    event
        .ts
        .signed_duration_since(chunk.ended_at)
        .num_seconds()
        .abs()
        <= 2
}

fn merge_level(left: &LogLevel, right: &LogLevel) -> LogLevel {
    if severity_rank(right) > severity_rank(left) {
        right.clone()
    } else {
        left.clone()
    }
}

fn severity_rank(level: &LogLevel) -> u8 {
    match level {
        LogLevel::Debug => 0,
        LogLevel::Info => 1,
        LogLevel::Warn => 2,
        LogLevel::Error => 3,
    }
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
    [LogSource::Serial, LogSource::Ssh, LogSource::Jlink]
        .into_iter()
        .filter(|source| events.iter().any(|e| e.source == *source))
        .collect()
}

pub(crate) fn format_source_label(source: &LogSource) -> &'static str {
    match source {
        LogSource::Serial => "serial",
        LogSource::Ssh => "ssh",
        LogSource::Jlink => "jlink",
        LogSource::System => "system",
        LogSource::Job => "job",
    }
}

fn format_stream_source(source: &LogSource, channel: &LogChannel) -> String {
    let source_label = format_source_label(source);
    let channel_label = match channel {
        LogChannel::Stdout => "stdout",
        LogChannel::Stderr => "stderr",
        LogChannel::System => "system",
    };
    if matches!(source, LogSource::System | LogSource::Job) || matches!(channel, LogChannel::System)
    {
        source_label.to_owned()
    } else {
        format!("{source_label}/{channel_label}")
    }
}

// ---------------------------------------------------------------------------
// Stream terminal view
// ---------------------------------------------------------------------------

fn view_stream_terminal(
    all_chunks: &[StreamChunk],
    visible_chunks: &[StreamChunk],
    stream_window: StreamWindow,
    stream_container_ref: NodeRef,
    on_stream_scroll: Callback<Event>,
    on_stream_select_start: Callback<Event>,
    on_stream_mouse_up: Callback<MouseEvent>,
    on_stream_key_up: Callback<KeyboardEvent>,
    selection_lock_active: bool,
    logs_fullscreen: bool,
) -> Html {
    html! {
        <div class="overflow-hidden rounded-[20px] border border-black/10 bg-[#111827] shadow-inner">
            <div class="flex flex-wrap items-center justify-between gap-3 border-b border-white/10 bg-white/5 px-4 py-3">
                <div class="flex flex-wrap items-center gap-2">
                    <span class="inline-flex rounded-full bg-white/10 px-3 py-1.5 text-xs font-medium text-slate-100">{"stream"}</span>
                    <span class="inline-flex rounded-full bg-white/10 px-3 py-1.5 text-xs font-medium text-slate-300">{format!("{} chunks", all_chunks.len())}</span>
                </div>
                <span class="font-mono text-xs text-slate-400">{
                    if selection_lock_active { "merged terminal view · selection frozen" } else { "merged terminal view" }
                }</span>
            </div>
            <div
                ref={stream_container_ref}
                class={classes!(
                    "min-w-0", "overflow-auto", "px-4", "py-4",
                    "font-mono", "text-sm", "text-slate-100",
                    "[overflow-anchor:none]",
                    if logs_fullscreen { Some("max-h-[calc(100vh-14rem)]") } else { Some("max-h-[70vh]") }
                )}
                onscroll={on_stream_scroll}
                onselectstart={on_stream_select_start}
                onmouseup={on_stream_mouse_up}
                onkeyup={on_stream_key_up}
            >
                <div style={format!("height: {}px;", stream_window.top_padding_px.max(0))}></div>
                {
                    for visible_chunks.iter().enumerate().map(|(i, chunk)| {
                        let idx = stream_window.start_index + i;
                        html! {
                            <div class={classes!(if idx > 0 { Some("mt-4 border-t border-white/10 pt-4") } else { None })}>
                                <div class="mb-3 flex flex-wrap items-center justify-between gap-3 text-xs text-slate-400">
                                    <div class="flex flex-wrap items-center gap-2">
                                        <span class="inline-flex rounded-full bg-white/10 px-3 py-1.5 font-mono text-xs font-medium text-slate-200">{chunk.device_id.clone()}</span>
                                        <span class={stream_badge_class(&chunk.channel)}>{format_stream_source(&chunk.source, &chunk.channel)}</span>
                                        <span class={stream_level_class(&chunk.level)}>{format!("{:?}", chunk.level)}</span>
                                    </div>
                                    <span>{
                                        if chunk.started_at == chunk.ended_at {
                                            chunk.started_at.to_rfc3339()
                                        } else {
                                            format!("{} -> {}", chunk.started_at.to_rfc3339(), chunk.ended_at.to_rfc3339())
                                        }
                                    }</span>
                                </div>
                                <pre class="m-0 min-w-0 whitespace-pre-wrap break-all text-sm text-slate-100">
                                    {render_stream_chunk_body(chunk)}
                                </pre>
                            </div>
                        }
                    })
                }
                <div style={format!("height: {}px;", stream_window.bottom_padding_px.max(0))}></div>
            </div>
        </div>
    }
}

fn render_stream_chunk_body(chunk: &StreamChunk) -> Html {
    if should_plain_render_stream_chunk(chunk) {
        let text = chunk
            .lines
            .iter()
            .map(|line| plain_log_text(line))
            .collect::<Vec<_>>()
            .join("\n");
        return html! { <span class={stream_body_class(&chunk.level)}>{text}</span> };
    }
    html! {
        for chunk.lines.iter().enumerate().map(|(i, line)| html! {
            <>
                {if i > 0 { html! {"\n"} } else { Html::default() }}
                {render_log_message(line, Some(stream_body_class(&chunk.level)))}
            </>
        })
    }
}

fn should_plain_render_stream_chunk(chunk: &StreamChunk) -> bool {
    if chunk.lines.len() >= STREAM_CHUNK_PLAIN_RENDER_LINE_THRESHOLD {
        return true;
    }
    chunk.lines.iter().map(|l| l.len()).sum::<usize>() >= STREAM_CHUNK_PLAIN_RENDER_CHAR_THRESHOLD
}

// ---------------------------------------------------------------------------
// Virtualization
// ---------------------------------------------------------------------------

fn compute_stream_window(
    chunks: &[StreamChunk],
    scroll_top: i32,
    viewport_height: i32,
    overscan_px: i32,
) -> StreamWindow {
    if chunks.is_empty() {
        return StreamWindow {
            start_index: 0,
            end_index: 0,
            top_padding_px: 0,
            bottom_padding_px: 0,
        };
    }
    if chunks.len() < STREAM_VIRTUALIZATION_MIN_CHUNKS {
        return StreamWindow {
            start_index: 0,
            end_index: chunks.len(),
            top_padding_px: 0,
            bottom_padding_px: 0,
        };
    }
    let scroll_start = scroll_top.saturating_sub(overscan_px).max(0);
    let scroll_end = scroll_top
        .saturating_add(viewport_height.max(STREAM_CHUNK_BASE_HEIGHT_PX))
        .saturating_add(overscan_px);

    let estimated_heights: Vec<i32> = chunks.iter().map(estimate_stream_chunk_height).collect();
    let total_height: i32 = estimated_heights.iter().sum();
    let mut traversed = 0;
    let mut start_index = chunks.len();
    let mut end_index = chunks.len();
    let mut top_padding_px = 0;

    for (i, &h) in estimated_heights.iter().enumerate() {
        let next = traversed + h;
        if start_index == chunks.len() && next >= scroll_start {
            start_index = i;
            top_padding_px = traversed;
        }
        if next >= scroll_end {
            end_index = i + 1;
            break;
        }
        traversed = next;
    }
    if start_index == chunks.len() {
        start_index = chunks.len().saturating_sub(1);
        top_padding_px = total_height - estimated_heights[start_index];
    }
    if end_index == chunks.len() {
        end_index = chunks.len();
    }
    let visible_height: i32 = estimated_heights[start_index..end_index].iter().sum();
    let bottom_padding_px = total_height - top_padding_px - visible_height;

    StreamWindow {
        start_index,
        end_index,
        top_padding_px,
        bottom_padding_px,
    }
}

fn estimate_stream_chunk_height(chunk: &StreamChunk) -> i32 {
    let plain_render = should_plain_render_stream_chunk(chunk);
    let line_count = chunk
        .lines
        .iter()
        .map(|line| {
            let rendered = if plain_render {
                plain_log_text(line)
            } else {
                visible_log_text(line)
            };
            estimate_visual_line_count(&rendered)
        })
        .sum::<usize>()
        .max(1) as i32;
    STREAM_CHUNK_BASE_HEIGHT_PX + (line_count * STREAM_CHUNK_LINE_HEIGHT_PX)
}

fn estimate_visual_line_count(line: &str) -> usize {
    let chars = line.chars().count().max(1);
    chars.div_ceil(STREAM_WRAP_WIDTH_CHARS)
}

// ---------------------------------------------------------------------------
// Anchor save / restore
// ---------------------------------------------------------------------------

fn capture_stream_anchor(chunks: &[StreamChunk], scroll_top: i32) -> Option<StreamAnchor> {
    if chunks.is_empty() {
        return None;
    }
    let target = scroll_top.max(0);
    let mut traversed = 0;
    for chunk in chunks {
        let h = estimate_stream_chunk_height(chunk);
        let next = traversed + h;
        if target < next {
            return Some(StreamAnchor {
                device_id: chunk.device_id.clone(),
                source: chunk.source.clone(),
                channel: chunk.channel.clone(),
                started_at: chunk.started_at,
                offset_px: target - traversed,
            });
        }
        traversed = next;
    }
    chunks.last().map(|c| StreamAnchor {
        device_id: c.device_id.clone(),
        source: c.source.clone(),
        channel: c.channel.clone(),
        started_at: c.started_at,
        offset_px: 0,
    })
}

fn restore_stream_anchor_scroll_top(chunks: &[StreamChunk], anchor: &StreamAnchor) -> Option<i32> {
    let mut traversed = 0;
    for chunk in chunks {
        if chunk.device_id == anchor.device_id
            && chunk.source == anchor.source
            && chunk.channel == anchor.channel
            && chunk.started_at == anchor.started_at
        {
            return Some(traversed + anchor.offset_px.max(0));
        }
        traversed += estimate_stream_chunk_height(chunk);
    }
    None
}

fn has_active_text_selection() -> bool {
    window()
        .and_then(|w| w.get_selection().ok().flatten())
        .is_some_and(|sel| !String::from(sel.to_string()).trim().is_empty())
}

fn set_page_scroll_locked(locked: bool) {
    if let Some(body) = window().and_then(|w| w.document()).and_then(|d| d.body()) {
        let style = body.style();
        if locked {
            let _ = style.set_property("overflow", "hidden");
        } else {
            let _ = style.remove_property("overflow");
        }
    }
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
        _ => "inline-flex rounded-full bg-black/5 px-3 py-1.5 text-xs font-medium text-ink-muted",
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

fn stream_badge_class(channel: &LogChannel) -> &'static str {
    match channel {
        LogChannel::Stdout => {
            "inline-flex rounded-full bg-emerald-500/15 px-3 py-1.5 text-xs font-medium text-emerald-300"
        }
        LogChannel::Stderr => {
            "inline-flex rounded-full bg-red-700/15 px-3 py-1.5 text-xs font-medium text-red-300"
        }
        LogChannel::System => {
            "inline-flex rounded-full bg-slate-500/15 px-3 py-1.5 text-xs font-medium text-slate-300"
        }
    }
}

fn stream_level_class(level: &LogLevel) -> &'static str {
    match level {
        LogLevel::Error => {
            "inline-flex rounded-full bg-red-700/10 px-3 py-1.5 text-xs font-medium text-red-300"
        }
        LogLevel::Warn => {
            "inline-flex rounded-full bg-amber-500/15 px-3 py-1.5 text-xs font-medium text-amber-200"
        }
        LogLevel::Info => {
            "inline-flex rounded-full bg-cyan-500/15 px-3 py-1.5 text-xs font-medium text-cyan-200"
        }
        LogLevel::Debug => {
            "inline-flex rounded-full bg-slate-500/15 px-3 py-1.5 text-xs font-medium text-slate-300"
        }
    }
}

fn stream_body_class(level: &LogLevel) -> &'static str {
    match level {
        LogLevel::Error => "text-red-300",
        LogLevel::Warn => "text-amber-200",
        LogLevel::Info => "text-slate-100",
        LogLevel::Debug => "text-slate-300",
    }
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
            ts: Utc
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
    fn merges_sustained_streaming_logs_into_shared_chunks() {
        let events = vec![
            sample_event(0, "boot line 1"),
            sample_event(1, "boot line 2"),
            sample_event(2, "boot line 3"),
        ];
        let chunks = build_stream_chunks(&events);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].lines.len(), 3);
    }

    #[test]
    fn does_not_merge_adjacent_events_from_different_devices() {
        let mut beta = sample_event(1, "boot line 2");
        beta.device_id = "board-beta".into();
        let events = vec![sample_event(0, "boot line 1"), beta];

        let chunks = build_stream_chunks(&events);

        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].device_id, "board-alpha");
        assert_eq!(chunks[1].device_id, "board-beta");
    }

    #[test]
    fn virtualizes_large_static_logs_to_an_overscanned_slice() {
        let chunks: Vec<StreamChunk> = (0..1_200)
            .map(|i| StreamChunk {
                device_id: format!("board-{i}"),
                source: LogSource::Serial,
                channel: LogChannel::Stdout,
                level: LogLevel::Info,
                started_at: Utc.timestamp_opt(i, 0).single().expect("valid"),
                ended_at: Utc.timestamp_opt(i, 0).single().expect("valid"),
                lines: vec![format!("line {i}")],
            })
            .collect();
        let window = compute_stream_window(&chunks, 12_000, 720, STREAM_VIRTUALIZATION_OVERSCAN_PX);
        assert!(window.start_index > 0);
        assert!(window.end_index < chunks.len());
        assert!(window.end_index - window.start_index < chunks.len());
        assert!(window.top_padding_px > 0);
        assert!(window.bottom_padding_px > 0);
    }

    #[test]
    fn keeps_moderate_chunk_sets_fully_rendered_to_avoid_scroll_gaps() {
        let chunks: Vec<StreamChunk> = (0..500)
            .map(|i| StreamChunk {
                device_id: format!("board-{i}"),
                source: LogSource::Serial,
                channel: LogChannel::Stdout,
                level: LogLevel::Info,
                started_at: Utc.timestamp_opt(i, 0).single().expect("valid"),
                ended_at: Utc.timestamp_opt(i, 0).single().expect("valid"),
                lines: vec![format!("line {i}")],
            })
            .collect();

        let window = compute_stream_window(&chunks, 12_000, 720, STREAM_VIRTUALIZATION_OVERSCAN_PX);

        assert_eq!(window.start_index, 0);
        assert_eq!(window.end_index, chunks.len());
        assert_eq!(window.top_padding_px, 0);
        assert_eq!(window.bottom_padding_px, 0);
    }

    #[test]
    fn keeps_small_chunk_sets_fully_rendered() {
        let chunks: Vec<StreamChunk> = (0..4)
            .map(|i| StreamChunk {
                device_id: format!("board-{i}"),
                source: LogSource::Serial,
                channel: LogChannel::Stdout,
                level: LogLevel::Info,
                started_at: Utc.timestamp_opt(i, 0).single().expect("valid"),
                ended_at: Utc.timestamp_opt(i, 0).single().expect("valid"),
                lines: vec![format!("line {i}")],
            })
            .collect();
        let window =
            compute_stream_window(&chunks, 999_999, 720, STREAM_VIRTUALIZATION_OVERSCAN_PX);
        assert_eq!(window.start_index, 0);
        assert_eq!(window.end_index, chunks.len());
        assert_eq!(window.top_padding_px, 0);
        assert_eq!(window.bottom_padding_px, 0);
    }

    #[test]
    fn long_wrapped_lines_increase_estimated_chunk_height() {
        let short = StreamChunk {
            device_id: "board-alpha".into(),
            source: LogSource::Serial,
            channel: LogChannel::Stdout,
            level: LogLevel::Info,
            started_at: Utc.timestamp_opt(0, 0).single().expect("valid"),
            ended_at: Utc.timestamp_opt(0, 0).single().expect("valid"),
            lines: vec!["short".into()],
        };
        let long = StreamChunk {
            lines: vec!["x".repeat(STREAM_WRAP_WIDTH_CHARS * 3)],
            ..short.clone()
        };
        assert!(estimate_stream_chunk_height(&long) > estimate_stream_chunk_height(&short));
    }

    #[test]
    fn rich_render_height_ignores_hidden_ansi_and_prefix_text() {
        let rendered = StreamChunk {
            device_id: "board-alpha".into(),
            source: LogSource::Serial,
            channel: LogChannel::Stdout,
            level: LogLevel::Info,
            started_at: Utc.timestamp_opt(0, 0).single().expect("valid"),
            ended_at: Utc.timestamp_opt(0, 0).single().expect("valid"),
            lines: vec!["\u{1b}[36m[I]\u{1b}[0m worker.loop: boot completed".into()],
        };
        let plain = StreamChunk {
            lines: vec!["boot completed".into()],
            ..rendered.clone()
        };

        assert_eq!(
            estimate_stream_chunk_height(&rendered),
            estimate_stream_chunk_height(&plain)
        );
    }

    #[test]
    fn restores_scroll_top_from_anchor_after_prefix_chunks_drop() {
        let chunks: Vec<StreamChunk> = (0..4)
            .map(|i| StreamChunk {
                device_id: format!("board-{i}"),
                source: LogSource::Serial,
                channel: LogChannel::Stdout,
                level: LogLevel::Info,
                started_at: Utc.timestamp_opt(i, 0).single().expect("valid"),
                ended_at: Utc.timestamp_opt(i, 0).single().expect("valid"),
                lines: vec![format!("line {i}")],
            })
            .collect();
        let anchor = capture_stream_anchor(&chunks, estimate_stream_chunk_height(&chunks[0]) + 8)
            .expect("anchor captured");
        let restored =
            restore_stream_anchor_scroll_top(&chunks[1..], &anchor).expect("anchor restored");
        assert_eq!(restored, 8);
    }

    #[test]
    fn anchor_returns_none_when_chunk_is_gone() {
        let chunks = vec![StreamChunk {
            device_id: "board-alpha".into(),
            source: LogSource::Serial,
            channel: LogChannel::Stdout,
            level: LogLevel::Info,
            started_at: Utc.timestamp_opt(1, 0).single().expect("valid"),
            ended_at: Utc.timestamp_opt(1, 0).single().expect("valid"),
            lines: vec!["line 1".into()],
        }];
        let anchor = StreamAnchor {
            device_id: "board-alpha".into(),
            source: LogSource::Serial,
            channel: LogChannel::Stdout,
            started_at: Utc.timestamp_opt(0, 0).single().expect("valid"),
            offset_px: 12,
        };
        assert_eq!(restore_stream_anchor_scroll_top(&chunks, &anchor), None);
    }

    #[test]
    fn large_chunks_switch_to_plain_rendering() {
        let chunk = StreamChunk {
            device_id: "board-alpha".into(),
            source: LogSource::Serial,
            channel: LogChannel::Stdout,
            level: LogLevel::Info,
            started_at: Utc.timestamp_opt(0, 0).single().expect("valid"),
            ended_at: Utc.timestamp_opt(0, 0).single().expect("valid"),
            lines: (0..STREAM_CHUNK_PLAIN_RENDER_LINE_THRESHOLD)
                .map(|i| format!("line {i}"))
                .collect(),
        };
        assert!(should_plain_render_stream_chunk(&chunk));
    }

    #[test]
    fn small_chunks_keep_rich_rendering() {
        let chunk = StreamChunk {
            device_id: "board-alpha".into(),
            source: LogSource::Serial,
            channel: LogChannel::Stdout,
            level: LogLevel::Info,
            started_at: Utc.timestamp_opt(0, 0).single().expect("valid"),
            ended_at: Utc.timestamp_opt(0, 0).single().expect("valid"),
            lines: vec!["boot ok".into(), "ready".into()],
        };
        assert!(!should_plain_render_stream_chunk(&chunk));
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
