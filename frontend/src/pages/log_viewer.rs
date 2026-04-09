use chrono::{DateTime, Utc};
use device_hub_shared::{LogChannel, LogEvent, LogLevel, LogSource};
use gloo_timers::callback::Timeout;
use wasm_bindgen::{JsCast, closure::Closure};
use web_sys::{
    ErrorEvent, Event, HtmlElement, HtmlInputElement, HtmlSelectElement, KeyboardEvent,
    MessageEvent, MouseEvent, WebSocket, window,
};
use yew::prelude::*;

use crate::api;
use crate::components::{
    log_event::{LogEventCard, render_log_message},
    serial_write_panel::SerialWritePanel,
};

#[derive(Properties, PartialEq)]
pub struct LogViewerProps {
    #[prop_or_default]
    pub device_id: Option<String>,
}

struct WsBindings {
    socket: WebSocket,
    _on_open: Closure<dyn FnMut(Event)>,
    _on_message: Closure<dyn FnMut(MessageEvent)>,
    _on_error: Closure<dyn FnMut(ErrorEvent)>,
    _on_close: Closure<dyn FnMut(Event)>,
}

const EVENT_PAGE_SIZE: usize = 50;
const DEFAULT_BUFFERED_EVENTS: usize = 2_000;
const RENDER_FLUSH_DELAY_MS: u32 = 80;
const STREAM_VIRTUALIZATION_MIN_CHUNKS: usize = 100;
const STREAM_VIRTUALIZATION_OVERSCAN_PX: i32 = 480;
const STREAM_CHUNK_BASE_HEIGHT_PX: i32 = 56;
const STREAM_CHUNK_LINE_HEIGHT_PX: i32 = 20;
const STREAM_WRAP_WIDTH_CHARS: usize = 120;

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
struct StreamChunk {
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

const STREAM_TAIL_LOCK_THRESHOLD_PX: i32 = 0;

#[function_component(LogViewerPage)]
pub fn log_viewer_page(props: &LogViewerProps) -> Html {
    let events = use_state(Vec::<LogEvent>::new);
    let buffered_event_limit = use_state(|| DEFAULT_BUFFERED_EVENTS);
    let current_page = use_state(|| 0_usize);
    let event_store = use_mut_ref(Vec::<LogEvent>::new);
    let pending_render = use_mut_ref(|| Option::<Timeout>::None);
    let device_filter = use_state(|| props.device_id.clone().unwrap_or_default());
    let job_filter = use_state(String::new);
    let search_query = use_state(String::new);
    let connection = use_state(|| "connecting".to_owned());
    let error = use_state(|| Option::<String>::None);
    let connect_revision = use_state(|| 0_u32);
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
    let socket = use_mut_ref(|| Option::<WsBindings>::None);

    {
        let device_filter = device_filter.clone();
        let initial_device_id = props.device_id.clone();
        use_effect_with(props.device_id.clone(), move |_| {
            device_filter.set(initial_device_id.unwrap_or_default());
            || {}
        });
    }

    {
        let events = events.clone();
        let current_page = current_page.clone();
        let event_store = event_store.clone();
        let device_filter = device_filter.clone();
        let job_filter = job_filter.clone();
        let connection = connection.clone();
        let error = error.clone();
        let buffered_event_limit = buffered_event_limit.clone();
        let pending_render = pending_render.clone();
        let socket = socket.clone();
        use_effect_with(*connect_revision, move |_| {
            if let Some(existing) = socket.borrow_mut().take() {
                let _ = existing.socket.close();
            }

            connection.set("connecting".to_owned());
            error.set(None);
            events.set(Vec::new());
            current_page.set(0);
            *event_store.borrow_mut() = Vec::new();
            pending_render.borrow_mut().take();

            match api::ws_url(Some(&device_filter), Some(&job_filter)) {
                Ok(url) => match WebSocket::new(&url) {
                    Ok(ws) => {
                        let open_state = connection.clone();
                        let on_open = Closure::wrap(Box::new(move |_event: Event| {
                            open_state.set("connected".to_owned());
                        })
                            as Box<dyn FnMut(_)>);
                        ws.set_onopen(Some(on_open.as_ref().unchecked_ref()));

                        let message_events = events.clone();
                        let message_store = event_store.clone();
                        let message_error = error.clone();
                        let buffered_event_limit = buffered_event_limit.clone();
                        let pending_render = pending_render.clone();
                        let on_message = Closure::wrap(Box::new(move |event: MessageEvent| {
                            if let Some(text) = event.data().as_string() {
                                match serde_json::from_str::<LogEvent>(&text) {
                                    Ok(log_event) => {
                                        let mut next = message_store.borrow_mut();
                                        if !next.iter().any(|existing| {
                                            existing.ts == log_event.ts
                                                && existing.device_id == log_event.device_id
                                                && existing.job_id == log_event.job_id
                                                && existing.source == log_event.source
                                                && existing.channel == log_event.channel
                                                && existing.level == log_event.level
                                                && existing.message == log_event.message
                                        }) {
                                            next.push(log_event);
                                            trim_buffered_events(&mut next, *buffered_event_limit);
                                            if pending_render.borrow().is_none() {
                                                let message_events = message_events.clone();
                                                let message_store = message_store.clone();
                                                let pending_render_for_timer =
                                                    pending_render.clone();
                                                let pending_render_for_callback =
                                                    pending_render.clone();
                                                *pending_render_for_timer.borrow_mut() =
                                                    Some(Timeout::new(
                                                        RENDER_FLUSH_DELAY_MS,
                                                        move || {
                                                            let snapshot =
                                                                message_store.borrow().clone();
                                                            message_events.set(snapshot);
                                                            pending_render_for_callback
                                                                .borrow_mut()
                                                                .take();
                                                        },
                                                    ));
                                            }
                                        }
                                    }
                                    Err(parse_error) => {
                                        message_error.set(Some(parse_error.to_string()));
                                    }
                                }
                            }
                        })
                            as Box<dyn FnMut(_)>);
                        ws.set_onmessage(Some(on_message.as_ref().unchecked_ref()));

                        let error_state = error.clone();
                        let on_error = Closure::wrap(Box::new(move |_event: ErrorEvent| {
                            error_state.set(Some("websocket stream error".to_owned()));
                        })
                            as Box<dyn FnMut(_)>);
                        ws.set_onerror(Some(on_error.as_ref().unchecked_ref()));

                        let close_state = connection.clone();
                        let on_close = Closure::wrap(Box::new(move |_event: Event| {
                            close_state.set("disconnected".to_owned());
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
                        connection.set("disconnected".to_owned());
                        error.set(Some("failed to open websocket".to_owned()));
                    }
                },
                Err(message) => {
                    connection.set("disconnected".to_owned());
                    error.set(Some(message));
                }
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

    let on_device_input = {
        let device_filter = device_filter.clone();
        Callback::from(move |event: InputEvent| {
            let input: HtmlInputElement = event.target_unchecked_into();
            device_filter.set(input.value());
        })
    };

    let on_job_input = {
        let job_filter = job_filter.clone();
        Callback::from(move |event: InputEvent| {
            let input: HtmlInputElement = event.target_unchecked_into();
            job_filter.set(input.value());
        })
    };

    let on_reconnect = {
        let connect_revision = connect_revision.clone();
        Callback::from(move |_| connect_revision.set(*connect_revision + 1))
    };

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
        Callback::from(move |event: Event| {
            let select: HtmlSelectElement = event.target_unchecked_into();
            if let Ok(next_limit) = select.value().parse::<usize>() {
                buffered_event_limit.set(next_limit);
            }
        })
    };

    let status_class = match connection.as_str() {
        "connected" => {
            "inline-flex rounded-full bg-accent/10 px-3 py-1.5 text-xs font-medium text-accent-strong"
        }
        "connecting" => {
            "inline-flex rounded-full bg-amber-500/15 px-3 py-1.5 text-xs font-medium text-amber-800"
        }
        _ => "inline-flex rounded-full bg-black/5 px-3 py-1.5 text-xs font-medium text-ink-muted",
    };
    let search_query_value = search_query.trim().to_ascii_lowercase();
    let matched_events = use_memo(
        (events.clone(), search_query_value.clone()),
        |(events, search_query_value)| {
            events
                .iter()
                .filter(|event| matches_search_query(event, search_query_value))
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
        |(events, stream_filter, search_query_value)| {
            events
                .iter()
                .filter(|event| {
                    matches_stream_filter(event, stream_filter)
                        && matches_search_query(event, search_query_value)
                })
                .cloned()
                .collect::<Vec<_>>()
        },
    );
    let filtered_stream_events = &*filtered_stream_events;
    let stream_chunks = use_memo(filtered_stream_events.clone(), |filtered_stream_events| {
        build_stream_chunks(filtered_stream_events)
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
        && props.device_id.is_some();

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
        Callback::from(move |_| {
            selection_lock_active.set(false);
            follow_stream_tail.set(true);
            if let Some(element) = stream_container_ref.cast::<HtmlElement>() {
                element.set_scroll_top(element.scroll_height());
                *last_stream_scroll_top.borrow_mut() = element.scroll_top();
                stream_scroll_top.set(element.scroll_top());
            }
        })
    };

    let on_filter_all = {
        let stream_filter = stream_filter.clone();
        Callback::from(move |_| stream_filter.set(StreamFilter::All))
    };

    let on_filter_device_logs = {
        let stream_filter = stream_filter.clone();
        Callback::from(move |_| stream_filter.set(StreamFilter::DeviceLogs))
    };

    let on_filter_structured = {
        let stream_filter = stream_filter.clone();
        Callback::from(move |_| stream_filter.set(StreamFilter::Structured))
    };

    let on_stream_scroll = {
        let stream_container_ref = stream_container_ref.clone();
        let follow_stream_tail = follow_stream_tail.clone();
        let stream_scroll_top = stream_scroll_top.clone();
        let stream_viewport_height = stream_viewport_height.clone();
        let last_stream_scroll_top = last_stream_scroll_top.clone();
        Callback::from(move |_| {
            if let Some(element) = stream_container_ref.cast::<HtmlElement>() {
                *last_stream_scroll_top.borrow_mut() = element.scroll_top();
                stream_scroll_top.set(element.scroll_top());
                stream_viewport_height.set(element.client_height());
                follow_stream_tail.set(is_stream_scrolled_to_bottom(
                    &element,
                    STREAM_TAIL_LOCK_THRESHOLD_PX,
                ));
            }
        })
    };

    {
        let events = events.clone();
        let event_store = event_store.clone();
        let current_page = current_page.clone();
        use_effect_with(*buffered_event_limit, move |buffered_event_limit| {
            let mut next = event_store.borrow().clone();
            trim_buffered_events(&mut next, *buffered_event_limit);
            *event_store.borrow_mut() = next.clone();
            events.set(next);
            current_page.set(0);
            || {}
        });
    }

    {
        let frozen_stream_chunks = frozen_stream_chunks.clone();
        let stream_chunks = (*stream_chunks).clone();
        let selection_lock_active = selection_lock_active.clone();
        use_effect_with((*selection_lock_active, stream_chunks.clone()), move |_| {
            if *selection_lock_active {
                if frozen_stream_chunks.is_none() {
                    frozen_stream_chunks.set(Some(stream_chunks.clone()));
                }
            } else if frozen_stream_chunks.is_some() {
                frozen_stream_chunks.set(None);
            }
            || {}
        });
    }

    {
        let stream_container_ref = stream_container_ref.clone();
        let follow_stream_tail = follow_stream_tail.clone();
        let stream_scroll_top = stream_scroll_top.clone();
        let stream_viewport_height = stream_viewport_height.clone();
        let selection_lock_active = selection_lock_active.clone();
        let last_stream_scroll_top = last_stream_scroll_top.clone();
        let view_mode_value = *view_mode;
        let stream_chunk_count = displayed_stream_chunks.len();
        let filtered_event_count = filtered_stream_events.len();
        use_effect_with(
            (
                view_mode_value,
                stream_chunk_count,
                filtered_event_count,
                *follow_stream_tail,
                *selection_lock_active,
            ),
            move |_| {
                if view_mode_value == LogViewMode::Stream {
                    if let Some(element) = stream_container_ref.cast::<HtmlElement>() {
                        stream_viewport_height.set(element.client_height());
                        if *selection_lock_active {
                            *last_stream_scroll_top.borrow_mut() = element.scroll_top();
                            stream_scroll_top.set(element.scroll_top());
                        } else if *follow_stream_tail {
                            element.set_scroll_top(element.scroll_height());
                            *last_stream_scroll_top.borrow_mut() = element.scroll_top();
                            stream_scroll_top.set(element.scroll_top());
                        } else {
                            element.set_scroll_top(*last_stream_scroll_top.borrow());
                            stream_scroll_top.set(element.scroll_top());
                        }
                    }
                }
                || {}
            },
        );
    }

    {
        let last_stream_scroll_top = last_stream_scroll_top.clone();
        let view_mode_value = *view_mode;
        let stream_filter_value = (*stream_filter).clone();
        use_effect_with((view_mode_value, stream_filter_value), move |_| {
            *last_stream_scroll_top.borrow_mut() = 0;
            || {}
        });
    }

    let on_stream_select_start = {
        let selection_lock_active = selection_lock_active.clone();
        Callback::from(move |_event: Event| selection_lock_active.set(true))
    };

    let on_stream_selection_change = {
        let selection_lock_active = selection_lock_active.clone();
        Callback::from(move |_event: MouseEvent| {
            selection_lock_active.set(has_active_text_selection());
        })
    };

    let on_stream_selection_key_up = {
        let selection_lock_active = selection_lock_active.clone();
        Callback::from(move |_event: KeyboardEvent| {
            selection_lock_active.set(has_active_text_selection());
        })
    };

    let on_toggle_logs_fullscreen = {
        let logs_fullscreen = logs_fullscreen.clone();
        Callback::from(move |_| logs_fullscreen.set(!*logs_fullscreen))
    };

    {
        let logs_fullscreen = logs_fullscreen.clone();
        use_effect_with(*logs_fullscreen, move |is_fullscreen| {
            set_page_scroll_locked(*is_fullscreen);
            || set_page_scroll_locked(false)
        });
    }

    html! {
        <section class="grid gap-5 xl:grid-cols-2">
            <article class="rounded-[24px] border border-black/10 bg-white/80 p-6 shadow-shell backdrop-blur xl:col-span-2">
                <span class="inline-flex items-center gap-2 uppercase tracking-[0.18em] text-[0.74rem] text-accent-strong">{"Live logs"}</span>
                <h2 class="mb-3 mt-3 text-3xl font-semibold tracking-tight text-ink">{"WebSocket event stream"}</h2>
                <p class="m-0 text-ink-muted">{
                    if props.device_id.is_some() {
                        "This stream is scoped to the selected device by default. You can still narrow further with a job filter or reconnect after changing filters."
                    } else {
                        "Subscribe to all events or narrow the stream to one device or one job. Reconnect after changing filters to open a new `/api/ws` subscription."
                    }
                }</p>
                <p class="mb-0 mt-2 text-ink-muted">{"Filters are combined: when both fields are set, events must match both device and job."}</p>
                <div class="mt-5 grid items-end gap-4 lg:grid-cols-3">
                    <label class="grid gap-2 text-sm text-ink-muted">
                        <span>{"Device filter"}</span>
                        <input class="w-full rounded-2xl border border-black/10 bg-white/75 px-4 py-3 text-ink outline-none transition placeholder:text-ink-muted/70 focus:border-accent/40 focus:ring-2 focus:ring-accent/10" value={(*device_filter).clone()} oninput={on_device_input} placeholder="board-alpha" readonly={props.device_id.is_some()} />
                    </label>
                    <label class="grid gap-2 text-sm text-ink-muted">
                        <span>{"Job filter"}</span>
                        <input class="w-full rounded-2xl border border-black/10 bg-white/75 px-4 py-3 text-ink outline-none transition placeholder:text-ink-muted/70 focus:border-accent/40 focus:ring-2 focus:ring-accent/10" value={(*job_filter).clone()} oninput={on_job_input} placeholder="job_123" />
                    </label>
                    <button class="rounded-2xl bg-gradient-to-br from-accent to-accent-strong px-4 py-3 font-medium text-white transition hover:opacity-95" onclick={on_reconnect}>{"Reconnect stream"}</button>
                </div>
                <div class="mt-4 grid gap-4 lg:grid-cols-[minmax(0,1fr)_14rem]">
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
                        <select class="w-full rounded-2xl border border-black/10 bg-white/75 px-4 py-3 text-ink outline-none transition focus:border-accent/40 focus:ring-2 focus:ring-accent/10" onchange={on_buffer_limit_change} value={buffered_event_limit.to_string()}>
                            <option value="500">{"500"}</option>
                            <option value="1000">{"1000"}</option>
                            <option value="2000">{"2000"}</option>
                            <option value="5000">{"5000"}</option>
                        </select>
                    </label>
                </div>
                <div class="mt-4 flex flex-wrap items-center gap-3">
                    <span class={status_class}>{(*connection).clone()}</span>
                    <span class="font-mono text-sm text-ink-muted">{format!("{} matched / {} buffered (cap {})", matched_events.len(), events.len(), *buffered_event_limit)}</span>
                </div>
                {
                    if let Some(message) = (*error).clone() {
                        html! { <p class="mb-0 mt-4 text-sm text-red-700">{message}</p> }
                    } else {
                        Html::default()
                    }
                }
            </article>
            <article class={classes!(
                "rounded-[24px]",
                "border",
                "border-black/10",
                "bg-white/80",
                "p-6",
                "shadow-shell",
                "backdrop-blur",
                "xl:col-span-2",
                if *logs_fullscreen {
                    Some("fixed inset-4 z-[70] overflow-auto bg-white/95")
                } else {
                    None
                }
            )}>
                <div class="mb-5 flex flex-wrap items-center justify-between gap-3">
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
                    </div>
                    <div class="flex flex-wrap items-center gap-2.5">
                        {
                            if *view_mode == LogViewMode::Stream {
                                html! {
                                    <div class="inline-flex rounded-full border border-black/10 bg-white/75 p-1">
                                        <button class={filter_button_class(*stream_filter == StreamFilter::All)} onclick={on_filter_all}>{"All"}</button>
                                        <button class={filter_button_class(*stream_filter == StreamFilter::DeviceLogs)} onclick={on_filter_device_logs}>{"Device logs"}</button>
                                        {
                                            for available_sources.iter().cloned().map(|source| {
                                                let stream_filter = stream_filter.clone();
                                                let active = *stream_filter == StreamFilter::Source(source.clone());
                                                let label = format_source_label(&source);
                                                let source_for_click = source.clone();
                                                let onclick = Callback::from(move |_| {
                                                    stream_filter.set(StreamFilter::Source(source_for_click.clone()));
                                                });
                                                html! {
                                                    <button class={filter_button_class(active)} {onclick}>{label}</button>
                                                }
                                            })
                                        }
                                        <button class={filter_button_class(*stream_filter == StreamFilter::Structured)} onclick={on_filter_structured}>{"Structured"}</button>
                                    </div>
                                }
                            } else {
                                Html::default()
                            }
                        }
                        <span class="font-mono text-sm text-ink-muted">{format!("page {} / {} · showing {} of {}", page + 1, total_pages, shown_events, total_events)}</span>
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
                            } else {
                                Html::default()
                            }
                        }
                        <button
                            class="rounded-2xl border border-black/10 bg-white/75 px-4 py-3 font-medium text-ink transition hover:bg-white"
                            onclick={on_toggle_logs_fullscreen.clone()}
                        >{
                            if *logs_fullscreen {
                                "Exit full screen"
                            } else {
                                "Full screen"
                            }
                        }</button>
                    </div>
                </div>
                <div class="grid gap-4">
                    {
                        if events.is_empty() {
                            html! { <p class="m-0 text-ink-muted">{"Waiting for live events..."}</p> }
                        } else if *view_mode == LogViewMode::Event {
                            html! {
                                for visible_events
                                    .iter()
                                    .rev()
                                    .cloned()
                                    .map(|event| html! { <LogEventCard event={event} /> })
                            }
                        } else if stream_chunks.is_empty() {
                            html! { <p class="m-0 text-ink-muted">{"No logs match the current stream filter on this page."}</p> }
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
                        html! { <SerialWritePanel device_id={props.device_id.clone().unwrap_or_default()} /> }
                    } else {
                        Html::default()
                    }
                }
            </article>
        </section>
    }
}

fn is_stream_scrolled_to_bottom(element: &HtmlElement, threshold_px: i32) -> bool {
    let distance_from_bottom =
        element.scroll_height() - element.scroll_top() - element.client_height();
    distance_from_bottom <= threshold_px.max(0)
}

fn matches_search_query(event: &LogEvent, query: &str) -> bool {
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
    .any(|value| value.to_ascii_lowercase().contains(query))
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

fn build_stream_chunks(events: &[LogEvent]) -> Vec<StreamChunk> {
    let mut chunks: Vec<StreamChunk> = Vec::new();

    for event in events {
        let normalized_message = event.message.trim_end_matches('\n').to_owned();
        if let Some(last) = chunks.last_mut() {
            if last.source == event.source
                && last.channel == event.channel
                && should_merge_stream_chunk(last, event)
            {
                last.ended_at = event.ts;
                last.level = merge_level(&last.level, &event.level);
                last.lines.push(normalized_message);
                continue;
            }
        }

        chunks.push(StreamChunk {
            source: event.source.clone(),
            channel: event.channel.clone(),
            level: event.level.clone(),
            started_at: event.ts,
            ended_at: event.ts,
            lines: vec![normalized_message],
        });
    }

    chunks
}

fn trim_buffered_events(events: &mut Vec<LogEvent>, max_events: usize) {
    if events.len() > max_events {
        let drain = events.len() - max_events;
        events.drain(0..drain);
    }
}

fn should_merge_stream_chunk(chunk: &StreamChunk, event: &LogEvent) -> bool {
    let is_structured_event = matches!(event.source, LogSource::System | LogSource::Job)
        || matches!(event.channel, LogChannel::System);
    if is_structured_event {
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
                    if selection_lock_active {
                        "merged terminal view · selection frozen"
                    } else {
                        "merged terminal view"
                    }
                }</span>
            </div>
            <div
                ref={stream_container_ref}
                class={classes!(
                    "min-w-0",
                    "overflow-auto",
                    "px-4",
                    "py-4",
                    "font-mono",
                    "text-sm",
                    "text-slate-100",
                    "[overflow-anchor:none]",
                    if logs_fullscreen {
                        Some("max-h-[calc(100vh-14rem)]")
                    } else {
                        Some("max-h-[70vh]")
                    }
                )}
                onscroll={on_stream_scroll}
                onselectstart={on_stream_select_start}
                onmouseup={on_stream_mouse_up}
                onkeyup={on_stream_key_up}
            >
                <div style={format!("height: {}px;", stream_window.top_padding_px.max(0))}></div>
                {
                    for visible_chunks.iter().enumerate().map(|(index, chunk)| {
                        let chunk_index = stream_window.start_index + index;
                        html! {
                            <div class={classes!(if chunk_index > 0 { Some("mt-4 border-t border-white/10 pt-4") } else { None })}>
                                <div class="mb-3 flex flex-wrap items-center justify-between gap-3 text-xs text-slate-400">
                                    <div class="flex flex-wrap items-center gap-2">
                                        <span class={stream_badge_class(&chunk.channel)}>{format_stream_source(&chunk.source, &chunk.channel)}</span>
                                        <span class={stream_level_class(&chunk.level)}>{format!("{:?}", chunk.level)}</span>
                                    </div>
                                    <span>
                                        {
                                            if chunk.started_at == chunk.ended_at {
                                                chunk.started_at.to_rfc3339()
                                            } else {
                                                format!("{} -> {}", chunk.started_at.to_rfc3339(), chunk.ended_at.to_rfc3339())
                                            }
                                        }
                                    </span>
                                </div>
                                <pre class="m-0 min-w-0 whitespace-pre-wrap break-all text-sm text-slate-100">
                                    {for chunk.lines.iter().enumerate().map(|(line_index, line)| html! {
                                        <>
                                            {if line_index > 0 { html! {"\n"} } else { Html::default() }}
                                            {render_log_message(line, Some(stream_body_class(&chunk.level)))}
                                        </>
                                    })}
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

    let estimated_heights = chunks
        .iter()
        .map(estimate_stream_chunk_height)
        .collect::<Vec<_>>();
    let total_height = estimated_heights.iter().sum::<i32>();
    let mut traversed_height = 0;
    let mut start_index = chunks.len();
    let mut end_index = chunks.len();
    let mut top_padding_px = 0;

    for (index, chunk_height) in estimated_heights.iter().enumerate() {
        let next_height = traversed_height + chunk_height;

        if start_index == chunks.len() && next_height >= scroll_start {
            start_index = index;
            top_padding_px = traversed_height;
        }

        if next_height >= scroll_end {
            end_index = index + 1;
            break;
        }

        traversed_height = next_height;
    }

    if start_index == chunks.len() {
        start_index = chunks.len().saturating_sub(1);
        top_padding_px = total_height - estimated_heights[start_index];
    }

    if end_index == chunks.len() {
        end_index = chunks.len();
    }

    let visible_height = estimated_heights[start_index..end_index]
        .iter()
        .sum::<i32>();
    let bottom_padding_px = total_height - top_padding_px - visible_height;

    StreamWindow {
        start_index,
        end_index,
        top_padding_px,
        bottom_padding_px,
    }
}

fn estimate_stream_chunk_height(chunk: &StreamChunk) -> i32 {
    let line_count = chunk
        .lines
        .iter()
        .map(|line| estimate_visual_line_count(line))
        .sum::<usize>()
        .max(1) as i32;

    STREAM_CHUNK_BASE_HEIGHT_PX + (line_count * STREAM_CHUNK_LINE_HEIGHT_PX)
}

fn estimate_visual_line_count(line: &str) -> usize {
    let char_count = line.chars().count().max(1);
    char_count.div_ceil(STREAM_WRAP_WIDTH_CHARS)
}

fn has_active_text_selection() -> bool {
    window()
        .and_then(|window| window.get_selection().ok().flatten())
        .is_some_and(|selection| !String::from(selection.to_string()).trim().is_empty())
}

fn set_page_scroll_locked(locked: bool) {
    if let Some(body) = window()
        .and_then(|window| window.document())
        .and_then(|document| document.body())
    {
        let style = body.style();
        if locked {
            let _ = style.set_property("overflow", "hidden");
        } else {
            let _ = style.remove_property("overflow");
        }
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
        .filter(|source| events.iter().any(|event| event.source == *source))
        .collect()
}

fn format_source_label(source: &LogSource) -> &'static str {
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
            .map(|index| sample_event(index, format!("line {index}")))
            .collect::<Vec<_>>();

        trim_buffered_events(&mut events, 3);

        assert_eq!(events.len(), 3);
        assert_eq!(events[0].message, "line 3");
        assert_eq!(events[2].message, "line 5");
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
    fn virtualizes_large_static_logs_to_an_overscanned_slice() {
        let chunks = (0..400)
            .map(|index| StreamChunk {
                source: LogSource::Serial,
                channel: LogChannel::Stdout,
                level: LogLevel::Info,
                started_at: Utc.timestamp_opt(index, 0).single().expect("valid start"),
                ended_at: Utc.timestamp_opt(index, 0).single().expect("valid end"),
                lines: vec![format!("line {index}")],
            })
            .collect::<Vec<_>>();

        let window = compute_stream_window(&chunks, 12_000, 720, STREAM_VIRTUALIZATION_OVERSCAN_PX);

        assert!(window.start_index > 0);
        assert!(window.end_index < chunks.len());
        assert!(window.end_index - window.start_index < 40);
        assert!(window.top_padding_px > 0);
        assert!(window.bottom_padding_px > 0);
    }

    #[test]
    fn keeps_small_chunk_sets_fully_rendered() {
        let chunks = (0..4)
            .map(|index| StreamChunk {
                source: LogSource::Serial,
                channel: LogChannel::Stdout,
                level: LogLevel::Info,
                started_at: Utc.timestamp_opt(index, 0).single().expect("valid start"),
                ended_at: Utc.timestamp_opt(index, 0).single().expect("valid end"),
                lines: vec![format!("line {index}")],
            })
            .collect::<Vec<_>>();

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
            source: LogSource::Serial,
            channel: LogChannel::Stdout,
            level: LogLevel::Info,
            started_at: Utc.timestamp_opt(0, 0).single().expect("valid start"),
            ended_at: Utc.timestamp_opt(0, 0).single().expect("valid end"),
            lines: vec!["short".into()],
        };
        let long = StreamChunk {
            lines: vec!["x".repeat(STREAM_WRAP_WIDTH_CHARS * 3)],
            ..short.clone()
        };

        assert!(estimate_stream_chunk_height(&long) > estimate_stream_chunk_height(&short));
    }
}
