use device_hub_shared::{
    Job, JobArtifact, JobEvidence, JobStatus, LogChannel, LogEvent, LogLevel, LogSource,
};
use wasm_bindgen::{JsCast, closure::Closure};
use wasm_bindgen_futures::spawn_local;
use web_sys::{ErrorEvent, Event, HtmlElement, MessageEvent, WebSocket};
use yew::prelude::*;
use yew_router::prelude::*;

use crate::components::{
    log_event::{LogEventCard, render_log_message},
    serial_write_panel::SerialWritePanel,
};
use crate::{api, app::Route};

#[derive(Properties, PartialEq)]
pub struct JobDetailProps {
    pub job_id: String,
}

struct WsBindings {
    socket: WebSocket,
    _on_open: Closure<dyn FnMut(Event)>,
    _on_message: Closure<dyn FnMut(MessageEvent)>,
    _on_error: Closure<dyn FnMut(ErrorEvent)>,
    _on_close: Closure<dyn FnMut(Event)>,
}

const EVENT_PAGE_SIZE: usize = 50;

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
    started_at: chrono::DateTime<chrono::Utc>,
    ended_at: chrono::DateTime<chrono::Utc>,
    lines: Vec<String>,
}

const STREAM_TAIL_LOCK_THRESHOLD_PX: i32 = 0;

#[function_component(JobDetailPage)]
pub fn job_detail_page(props: &JobDetailProps) -> Html {
    let job = use_state(|| Option::<Job>::None);
    let evidence = use_state(|| Option::<JobEvidence>::None);
    let events = use_state(Vec::<LogEvent>::new);
    let current_page = use_state(|| 0_usize);
    let event_store = use_mut_ref(Vec::<LogEvent>::new);
    let error = use_state(|| Option::<String>::None);
    let connection = use_state(|| "connecting".to_owned());
    let search_query = use_state(String::new);
    let view_mode = use_state(|| LogViewMode::Stream);
    let stream_filter = use_state(|| StreamFilter::DeviceLogs);
    let follow_stream_tail = use_state(|| true);
    let stream_container_ref = use_node_ref();
    let last_stream_scroll_top = use_mut_ref(|| 0_i32);
    let socket = use_mut_ref(|| Option::<WsBindings>::None);
    let rerun_pending = use_state(|| false);
    let navigator = use_navigator();

    {
        let job = job.clone();
        let evidence = evidence.clone();
        let events = events.clone();
        let current_page = current_page.clone();
        let event_store = event_store.clone();
        let error = error.clone();
        let connection = connection.clone();
        let socket = socket.clone();
        let job_id = props.job_id.clone();
        use_effect_with(props.job_id.clone(), move |_| {
            if let Some(existing) = socket.borrow_mut().take() {
                let _ = existing.socket.close();
            }

            events.set(Vec::new());
            current_page.set(0);
            *event_store.borrow_mut() = Vec::new();
            evidence.set(None);
            error.set(None);
            connection.set("connecting".to_owned());

            let job = job.clone();
            let evidence = evidence.clone();
            let events = events.clone();
            let event_store = event_store.clone();
            let error = error.clone();
            let connection = connection.clone();
            let socket_for_task = socket.clone();
            spawn_local(async move {
                match api::get_job_evidence(&job_id).await {
                    Ok(value) => {
                        job.set(Some(value.job.clone()));
                        evidence.set(Some(value));
                    }
                    Err(err) => {
                        error.set(Some(err));
                        connection.set("disconnected".to_owned());
                        return;
                    }
                }

                match api::get_job_events(&job_id).await {
                    Ok(value) => {
                        *event_store.borrow_mut() = value.clone();
                        events.set(value);
                    }
                    Err(err) => {
                        error.set(Some(err));
                        connection.set("disconnected".to_owned());
                        return;
                    }
                }

                match api::ws_url(None, Some(&job_id)) {
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
                                                    && existing.message == log_event.message
                                                    && existing.channel == log_event.channel
                                                    && existing.level == log_event.level
                                            }) {
                                                next.push(log_event);
                                                next.sort_by(|left, right| left.ts.cmp(&right.ts));
                                                message_events.set(next.clone());
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
                                error_state.set(Some("job websocket stream error".to_owned()));
                            })
                                as Box<dyn FnMut(_)>);
                            ws.set_onerror(Some(on_error.as_ref().unchecked_ref()));

                            let close_state = connection.clone();
                            let on_close = Closure::wrap(Box::new(move |_event: Event| {
                                close_state.set("disconnected".to_owned());
                            })
                                as Box<dyn FnMut(_)>);
                            ws.set_onclose(Some(on_close.as_ref().unchecked_ref()));

                            *socket_for_task.borrow_mut() = Some(WsBindings {
                                socket: ws,
                                _on_open: on_open,
                                _on_message: on_message,
                                _on_error: on_error,
                                _on_close: on_close,
                            });
                        }
                        Err(_) => {
                            connection.set("disconnected".to_owned());
                            error.set(Some("failed to open job websocket".to_owned()));
                        }
                    },
                    Err(message) => {
                        connection.set("disconnected".to_owned());
                        error.set(Some(message));
                    }
                }
            });

            let socket = socket.clone();
            move || {
                if let Some(existing) = socket.borrow_mut().take() {
                    let _ = existing.socket.close();
                }
            }
        });
    }

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
    let matched_events = events
        .iter()
        .filter(|event| matches_search_query(event, &search_query_value))
        .cloned()
        .collect::<Vec<_>>();
    let total_events = matched_events.len();
    let total_pages = total_events.max(1).div_ceil(EVENT_PAGE_SIZE);
    let page = (*current_page).min(total_pages.saturating_sub(1));
    let end_index = total_events.saturating_sub(page * EVENT_PAGE_SIZE);
    let start_index = end_index.saturating_sub(EVENT_PAGE_SIZE);
    let shown_events = end_index.saturating_sub(start_index);
    let visible_events = matched_events[start_index..end_index].to_vec();
    let filtered_stream_events = events
        .iter()
        .filter(|event| {
            matches_stream_filter(event, &stream_filter)
                && matches_search_query(event, &search_query_value)
        })
        .cloned()
        .collect::<Vec<_>>();
    let stream_chunks = build_stream_chunks(&filtered_stream_events);
    let available_sources = available_device_sources(events.as_ref());
    let job_evidence = (*evidence).clone();
    let serial_write_device_id = job
        .as_ref()
        .as_ref()
        .map(|value| value.device_id.clone())
        .filter(|value| !value.is_empty());
    let show_serial_write_panel = *view_mode == LogViewMode::Stream
        && matches!(&*stream_filter, StreamFilter::Source(LogSource::Serial))
        && serial_write_device_id.is_some();

    let on_rerun = {
        let navigator = navigator.clone();
        let rerun_pending = rerun_pending.clone();
        let error = error.clone();
        let job_id = props.job_id.clone();
        Callback::from(move |_| {
            let navigator = navigator.clone();
            let rerun_pending = rerun_pending.clone();
            let error = error.clone();
            let job_id = job_id.clone();
            rerun_pending.set(true);
            spawn_local(async move {
                match api::rerun_job(&job_id).await {
                    Ok(job) => {
                        rerun_pending.set(false);
                        error.set(None);
                        if let Some(navigator) = navigator {
                            navigator.push(&Route::JobDetail { job_id: job.id });
                        }
                    }
                    Err(message) => {
                        rerun_pending.set(false);
                        error.set(Some(message));
                    }
                }
            });
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

    let on_search_input = {
        let search_query = search_query.clone();
        let current_page = current_page.clone();
        Callback::from(move |event: InputEvent| {
            let input: web_sys::HtmlInputElement = event.target_unchecked_into();
            search_query.set(input.value());
            current_page.set(0);
        })
    };

    let on_jump_to_latest = {
        let follow_stream_tail = follow_stream_tail.clone();
        let stream_container_ref = stream_container_ref.clone();
        let last_stream_scroll_top = last_stream_scroll_top.clone();
        Callback::from(move |_| {
            follow_stream_tail.set(true);
            if let Some(element) = stream_container_ref.cast::<HtmlElement>() {
                element.set_scroll_top(element.scroll_height());
                *last_stream_scroll_top.borrow_mut() = element.scroll_top();
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
        let last_stream_scroll_top = last_stream_scroll_top.clone();
        Callback::from(move |_| {
            if let Some(element) = stream_container_ref.cast::<HtmlElement>() {
                *last_stream_scroll_top.borrow_mut() = element.scroll_top();
                follow_stream_tail.set(is_stream_scrolled_to_bottom(
                    &element,
                    STREAM_TAIL_LOCK_THRESHOLD_PX,
                ));
            }
        })
    };

    {
        let stream_container_ref = stream_container_ref.clone();
        let follow_stream_tail = follow_stream_tail.clone();
        let last_stream_scroll_top = last_stream_scroll_top.clone();
        let view_mode_value = *view_mode;
        let stream_chunk_count = stream_chunks.len();
        let filtered_event_count = filtered_stream_events.len();
        use_effect_with(
            (
                view_mode_value,
                stream_chunk_count,
                filtered_event_count,
                *follow_stream_tail,
            ),
            move |_| {
                if view_mode_value == LogViewMode::Stream {
                    if let Some(element) = stream_container_ref.cast::<HtmlElement>() {
                        if *follow_stream_tail {
                            element.set_scroll_top(element.scroll_height());
                            *last_stream_scroll_top.borrow_mut() = element.scroll_top();
                        } else {
                            element.set_scroll_top(*last_stream_scroll_top.borrow());
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

    html! {
        <section class="grid gap-5">
            <article class="min-w-0 rounded-[24px] border border-black/10 bg-white/80 p-6 shadow-shell backdrop-blur">
                <span class="inline-flex items-center gap-2 uppercase tracking-[0.18em] text-[0.74rem] text-accent-strong">{"Job"}</span>
                {
                    if let Some(message) = (*error).clone() {
                        html! { <p class="mb-0 mt-4 text-sm text-red-700">{message}</p> }
                    } else if let Some(job) = (*job).clone() {
                        let job_id = job.id.clone();
                        html! {
                            <>
                                <h2 class="mb-2 mt-3 text-3xl font-semibold tracking-tight text-ink">{job.action}</h2>
                                <p class="m-0 font-mono text-sm text-ink-muted">{job_id.clone()}</p>
                                 <div class="mt-4 flex flex-wrap items-center gap-3">
                                     <span class={job_status_class(&job.status)}>{format!("{:?}", job.status)}</span>
                                     <span class={status_class}>{(*connection).clone()}</span>
                                     <span class="inline-flex rounded-full bg-black/5 px-3 py-1.5 font-mono text-xs text-ink-muted">{format!("timeout {}s", job.timeout_secs)}</span>
                                 </div>
                                 <div class="mt-4 flex flex-wrap gap-3">
                                     <button
                                         class="rounded-2xl border border-black/10 bg-white/75 px-4 py-3 text-sm font-medium text-ink transition hover:bg-white disabled:cursor-default disabled:opacity-55"
                                         onclick={on_rerun.clone()}
                                         disabled={*rerun_pending}
                                     >{
                                         if *rerun_pending {
                                             "Rerunning..."
                                         } else {
                                             "Rerun with same inputs"
                                         }
                                     }</button>
                                 </div>
                                 {
                                     if let Some(evidence) = job_evidence.clone() {
                                         html! {
                                             <div class="mt-4 grid gap-4 xl:grid-cols-[minmax(0,1.2fr)_minmax(18rem,0.8fr)]">
                                                 <div class="rounded-[20px] border border-black/10 bg-black/[0.02] p-4">
                                                     <h3 class="m-0 text-lg font-semibold text-ink">{"Evidence summary"}</h3>
                                                     <p class="mb-0 mt-3 text-sm text-ink-muted">{evidence.summary.headline.clone()}</p>
                                                     {
                                                         if let Some(request) = evidence.summary.request.clone() {
                                                             html! { <p class="mb-0 mt-2 font-mono text-sm text-ink">{request}</p> }
                                                         } else {
                                                             Html::default()
                                                         }
                                                     }
                                                     {
                                                         if let Some(outcome) = evidence.summary.outcome.clone() {
                                                             html! { <p class="mb-0 mt-2 text-sm text-ink">{outcome}</p> }
                                                         } else {
                                                             Html::default()
                                                         }
                                                     }
                                                     <div class="mt-3 grid gap-2">
                                                         {for evidence.summary.highlights.iter().map(|line| html! {
                                                             <p class="m-0 text-sm text-ink-muted">{line}</p>
                                                         })}
                                                     </div>
                                                 </div>
                                                 <div class="rounded-[20px] border border-black/10 bg-black/[0.02] p-4">
                                                     <h3 class="m-0 text-lg font-semibold text-ink">{"Artifacts"}</h3>
                                                     <div class="mt-3 grid gap-2">
                                                         {render_artifacts(&evidence.artifacts)}
                                                     </div>
                                                 </div>
                                             </div>
                                         }
                                     } else if is_terminal_status(&job.status) {
                                         html! {
                                             <p class="mb-0 mt-4 text-ink-muted">
                                                 {"Loading persisted evidence summary and artifact metadata..."}
                                             </p>
                                         }
                                     } else {
                                         html! {
                                             <p class="mb-0 mt-4 text-ink-muted">
                                                 {"Live events stream below. The runner persists job metadata and event history continuously, then exposes artifact downloads and a rerun entrypoint when you need another attempt."}
                                             </p>
                                         }
                                     }
                                 }
                             </>
                        }
                    } else {
                        html! { <p class="mb-0 mt-4 text-ink-muted">{"Loading job..."}</p> }
                    }
                }
            </article>
            <article class="rounded-[24px] border border-black/10 bg-white/80 p-6 shadow-shell backdrop-blur">
                <div class="mb-4">
                    <label class="grid gap-2 text-sm text-ink-muted">
                        <span>{"Search logs"}</span>
                        <input
                            class="w-full rounded-2xl border border-black/10 bg-white/75 px-4 py-3 text-ink outline-none transition placeholder:text-ink-muted/70 focus:border-accent/40 focus:ring-2 focus:ring-accent/10"
                            value={(*search_query).clone()}
                            oninput={on_search_input}
                            placeholder="message, source, level, device_id, job_id"
                        />
                    </label>
                </div>
                <div class="flex flex-wrap items-center justify-between gap-3">
                    <span class="inline-flex items-center gap-2 uppercase tracking-[0.18em] text-[0.74rem] text-accent-strong">{"Logs"}</span>
                    <div class="inline-flex rounded-full border border-black/10 bg-white/75 p-1">
                        <button
                            class={classes!(
                                "rounded-full",
                                "px-3",
                                "py-1.5",
                                "text-sm",
                                "font-medium",
                                "transition",
                                if *view_mode == LogViewMode::Stream {
                                    "bg-accent text-white"
                                } else {
                                    "text-ink-muted hover:text-ink"
                                }
                            )}
                            onclick={on_show_stream_view}
                        >{"Stream"}</button>
                        <button
                            class={classes!(
                                "rounded-full",
                                "px-3",
                                "py-1.5",
                                "text-sm",
                                "font-medium",
                                "transition",
                                if *view_mode == LogViewMode::Event {
                                    "bg-accent text-white"
                                } else {
                                    "text-ink-muted hover:text-ink"
                                }
                            )}
                            onclick={on_show_event_view}
                        >{"Event"}</button>
                    </div>
                </div>
                <div class="mb-5 mt-4 flex flex-wrap items-center justify-between gap-3">
                    <span class="font-mono text-sm text-ink-muted">{format!("{} matched · page {} / {} · showing {} of {}", matched_events.len(), page + 1, total_pages, shown_events, total_events)}</span>
                    <div class="flex flex-wrap items-center gap-2.5">
                        {
                            if *view_mode == LogViewMode::Stream {
                                html! {
                                    <div class="inline-flex rounded-full border border-black/10 bg-white/75 p-1">
                                        <button
                                            class={filter_button_class(*stream_filter == StreamFilter::All)}
                                            onclick={on_filter_all}
                                        >{"All"}</button>
                                        <button
                                            class={filter_button_class(*stream_filter == StreamFilter::DeviceLogs)}
                                            onclick={on_filter_device_logs}
                                        >{"Device logs"}</button>
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
                                                    <button
                                                        class={filter_button_class(active)}
                                                        {onclick}
                                                    >{label}</button>
                                                }
                                            })
                                        }
                                        <button
                                            class={filter_button_class(*stream_filter == StreamFilter::Structured)}
                                            onclick={on_filter_structured}
                                        >{"Structured"}</button>
                                    </div>
                                }
                            } else {
                                Html::default()
                            }
                        }
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
                                        <span class="font-mono text-sm text-ink-muted">{format!("{} events -> {} chunks", filtered_stream_events.len(), stream_chunks.len())}</span>
                                        <button
                                            class="rounded-2xl border border-black/10 bg-white/75 px-4 py-3 font-medium text-ink transition hover:bg-white disabled:cursor-default disabled:opacity-55"
                                            onclick={on_jump_to_latest}
                                            disabled={*follow_stream_tail}
                                        >{"Jump to latest"}</button>
                                    </>
                                }
                            }
                        }
                    </div>
                </div>
                <div class="min-w-0 grid gap-4">
                    {
                        if events.is_empty() {
                            html! { <p class="m-0 text-ink-muted">{"Waiting for job events..."}</p> }
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
                                {view_stream_terminal(&stream_chunks, stream_container_ref.clone(), on_stream_scroll.clone())}
                            }
                        }
                    }
                </div>
                {
                    if show_serial_write_panel {
                        html! {
                            <SerialWritePanel device_id={serial_write_device_id.unwrap_or_default()} />
                        }
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

fn job_status_class(status: &JobStatus) -> &'static str {
    match status {
        JobStatus::Succeeded => {
            "inline-flex rounded-full bg-accent/10 px-3 py-1.5 text-xs font-medium text-accent-strong"
        }
        JobStatus::Failed => {
            "inline-flex rounded-full bg-red-700/10 px-3 py-1.5 text-xs font-medium text-red-700"
        }
        JobStatus::Cancelled => {
            "inline-flex rounded-full bg-black/5 px-3 py-1.5 text-xs font-medium text-ink-muted"
        }
        JobStatus::Pending | JobStatus::Running => {
            "inline-flex rounded-full bg-amber-500/15 px-3 py-1.5 text-xs font-medium text-amber-800"
        }
    }
}

fn is_terminal_status(status: &JobStatus) -> bool {
    matches!(
        status,
        JobStatus::Succeeded | JobStatus::Failed | JobStatus::Cancelled
    )
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
    chunks: &[StreamChunk],
    stream_container_ref: NodeRef,
    on_stream_scroll: Callback<Event>,
) -> Html {
    html! {
        <div class="overflow-hidden rounded-[20px] border border-black/10 bg-[#111827] shadow-inner">
            <div class="flex flex-wrap items-center justify-between gap-3 border-b border-white/10 bg-white/5 px-4 py-3">
                <div class="flex flex-wrap items-center gap-2">
                    <span class="inline-flex rounded-full bg-white/10 px-3 py-1.5 text-xs font-medium text-slate-100">{"stream"}</span>
                    <span class="inline-flex rounded-full bg-white/10 px-3 py-1.5 text-xs font-medium text-slate-300">{format!("{} chunks", chunks.len())}</span>
                </div>
                <span class="font-mono text-xs text-slate-400">{"merged terminal view"}</span>
            </div>
            <div ref={stream_container_ref} class="min-w-0 max-h-[70vh] overflow-auto px-4 py-4 font-mono text-sm text-slate-100 [overflow-anchor:none]" onscroll={on_stream_scroll}>
                {
                    for chunks.iter().enumerate().map(|(index, chunk)| {
                        html! {
                            <div class={classes!(if index > 0 { Some("mt-4 border-t border-white/10 pt-4") } else { None })}>
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
            </div>
        </div>
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

fn filter_button_class(active: bool) -> Classes {
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

fn format_stream_source(source: &LogSource, channel: &LogChannel) -> String {
    let source_label = match source {
        LogSource::Ssh => "ssh",
        LogSource::Serial => "serial",
        LogSource::Jlink => "jlink",
        LogSource::System => "system",
        LogSource::Job => "job",
    };
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

fn stream_level_class(level: &LogLevel) -> &'static str {
    match level {
        LogLevel::Error => {
            "inline-flex rounded-full bg-red-700/10 px-3 py-1.5 text-xs font-medium text-red-700"
        }
        LogLevel::Warn => {
            "inline-flex rounded-full bg-amber-500/15 px-3 py-1.5 text-xs font-medium text-amber-800"
        }
        LogLevel::Info => {
            "inline-flex rounded-full bg-cyan-500/15 px-3 py-1.5 text-xs font-medium text-cyan-700"
        }
        LogLevel::Debug => {
            "inline-flex rounded-full bg-slate-500/15 px-3 py-1.5 text-xs font-medium text-slate-700"
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

impl Drop for WsBindings {
    fn drop(&mut self) {
        let _ = self.socket.close();
        self.socket.set_onopen(None);
        self.socket.set_onmessage(None);
        self.socket.set_onerror(None);
        self.socket.set_onclose(None);
    }
}

fn render_artifacts(artifacts: &[JobArtifact]) -> Html {
    if artifacts.is_empty() {
        return html! { <p class="m-0 text-sm text-ink-muted">{"No artifacts persisted yet."}</p> };
    }

    html! {
        for artifacts.iter().map(|artifact| html! {
            <a
                class="flex items-center justify-between gap-3 rounded-2xl border border-black/10 bg-white/75 px-4 py-3 text-sm text-ink transition hover:bg-white"
                href={api::artifact_url(&artifact.download_url)}
                target="_blank"
                rel="noreferrer"
            >
                <span class="font-mono">{artifact.name.clone()}</span>
                <span class="text-ink-muted">{format_bytes(artifact.size_bytes)}</span>
            </a>
        })
    }
}

fn format_bytes(size_bytes: u64) -> String {
    if size_bytes >= 1024 * 1024 {
        format!("{:.1} MiB", size_bytes as f64 / (1024.0 * 1024.0))
    } else if size_bytes >= 1024 {
        format!("{:.1} KiB", size_bytes as f64 / 1024.0)
    } else {
        format!("{size_bytes} B")
    }
}
