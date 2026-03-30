use device_hub_shared::{Job, JobStatus, LogEvent};
use wasm_bindgen::{JsCast, closure::Closure};
use wasm_bindgen_futures::spawn_local;
use web_sys::{ErrorEvent, Event, MessageEvent, WebSocket};
use yew::prelude::*;

use crate::api;
use crate::components::log_event::LogEventCard;

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

#[function_component(JobDetailPage)]
pub fn job_detail_page(props: &JobDetailProps) -> Html {
    let job = use_state(|| Option::<Job>::None);
    let events = use_state(Vec::<LogEvent>::new);
    let current_page = use_state(|| 0_usize);
    let event_store = use_mut_ref(Vec::<LogEvent>::new);
    let error = use_state(|| Option::<String>::None);
    let connection = use_state(|| "connecting".to_owned());
    let socket = use_mut_ref(|| Option::<WsBindings>::None);

    {
        let job = job.clone();
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
            error.set(None);
            connection.set("connecting".to_owned());

            let job = job.clone();
            let events = events.clone();
            let event_store = event_store.clone();
            let error = error.clone();
            let connection = connection.clone();
            let socket_for_task = socket.clone();
            spawn_local(async move {
                match api::get_job(&job_id).await {
                    Ok(value) => job.set(Some(value)),
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
        "connected" => "inline-flex rounded-full bg-accent/10 px-3 py-1.5 text-xs font-medium text-accent-strong",
        "connecting" => "inline-flex rounded-full bg-amber-500/15 px-3 py-1.5 text-xs font-medium text-amber-800",
        _ => "inline-flex rounded-full bg-black/5 px-3 py-1.5 text-xs font-medium text-ink-muted",
    };
    let total_events = events.len();
    let total_pages = total_events.max(1).div_ceil(EVENT_PAGE_SIZE);
    let page = (*current_page).min(total_pages.saturating_sub(1));
    let end_index = total_events.saturating_sub(page * EVENT_PAGE_SIZE);
    let start_index = end_index.saturating_sub(EVENT_PAGE_SIZE);
    let shown_events = end_index.saturating_sub(start_index);

    let on_show_older = {
        let current_page = current_page.clone();
        Callback::from(move |_| current_page.set(*current_page + 1))
    };

    let on_show_newer = {
        let current_page = current_page.clone();
        Callback::from(move |_| current_page.set(current_page.saturating_sub(1)))
    };

    html! {
        <section class="grid gap-5 xl:grid-cols-2">
            <article class="rounded-[24px] border border-black/10 bg-white/80 p-6 shadow-shell backdrop-blur">
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
                                </div>
                                {
                                    if is_terminal_status(&job.status) {
                                        html! {
                                            <p class="mb-0 mt-4 text-ink-muted">
                                                {"Artifacts are persisted on the server as "}
                                                <code>{format!("jobs/{}/job.json", job_id)}</code>
                                                {" and "}
                                                <code>{format!("jobs/{}/events.ndjson", job_id)}</code>
                                                {" under the configured artifact root. Download/export is not available yet."}
                                            </p>
                                        }
                                    } else {
                                        html! {
                                            <p class="mb-0 mt-4 text-ink-muted">
                                                {"Live events stream below. Completed jobs keep "}
                                                <code>{"job.json"}</code>
                                                {" and "}
                                                <code>{"events.ndjson"}</code>
                                                {" under the server artifact root so history survives the in-memory buffer."}
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
                <span class="inline-flex items-center gap-2 uppercase tracking-[0.18em] text-[0.74rem] text-accent-strong">{"Events"}</span>
                <div class="mb-5 mt-4 flex flex-wrap items-center justify-between gap-3">
                    <span class="font-mono text-sm text-ink-muted">{format!("page {} / {} · showing {} of {}", page + 1, total_pages, shown_events, total_events)}</span>
                    <div class="flex flex-wrap gap-2.5">
                        <button class="rounded-2xl border border-black/10 bg-white/75 px-4 py-3 font-medium text-ink transition hover:bg-white disabled:cursor-default disabled:opacity-55" onclick={on_show_newer} disabled={page == 0}>{"Newer"}</button>
                        <button class="rounded-2xl border border-black/10 bg-white/75 px-4 py-3 font-medium text-ink transition hover:bg-white disabled:cursor-default disabled:opacity-55" onclick={on_show_older} disabled={start_index == 0 || total_events == 0}>{"Older"}</button>
                    </div>
                </div>
                <div class="grid gap-4">
                    {
                        if events.is_empty() {
                            html! { <p class="m-0 text-ink-muted">{"Waiting for job events..."}</p> }
                        } else {
                            html! {
                                for (*events)[start_index..]
                                    .iter()
                                    .rev()
                                    .cloned()
                                    .map(|event| html! { <LogEventCard event={event} /> })
                            }
                        }
                    }
                </div>
            </article>
        </section>
    }
}

fn job_status_class(status: &JobStatus) -> &'static str {
    match status {
        JobStatus::Succeeded => "inline-flex rounded-full bg-accent/10 px-3 py-1.5 text-xs font-medium text-accent-strong",
        JobStatus::Failed => "inline-flex rounded-full bg-red-700/10 px-3 py-1.5 text-xs font-medium text-red-700",
        JobStatus::Cancelled => "inline-flex rounded-full bg-black/5 px-3 py-1.5 text-xs font-medium text-ink-muted",
        JobStatus::Pending | JobStatus::Running => "inline-flex rounded-full bg-amber-500/15 px-3 py-1.5 text-xs font-medium text-amber-800",
    }
}

fn is_terminal_status(status: &JobStatus) -> bool {
    matches!(
        status,
        JobStatus::Succeeded | JobStatus::Failed | JobStatus::Cancelled
    )
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
