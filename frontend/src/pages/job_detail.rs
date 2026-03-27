use device_hub_shared::{Job, JobStatus, LogChannel, LogEvent, LogLevel};
use wasm_bindgen::{JsCast, closure::Closure};
use wasm_bindgen_futures::spawn_local;
use web_sys::{ErrorEvent, Event, MessageEvent, WebSocket};
use yew::prelude::*;

use crate::api;

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

#[function_component(JobDetailPage)]
pub fn job_detail_page(props: &JobDetailProps) -> Html {
    let job = use_state(|| Option::<Job>::None);
    let events = use_state(Vec::<LogEvent>::new);
    let error = use_state(|| Option::<String>::None);
    let connection = use_state(|| "connecting".to_owned());
    let socket = use_mut_ref(|| Option::<WsBindings>::None);

    {
        let job = job.clone();
        let events = events.clone();
        let error = error.clone();
        let connection = connection.clone();
        let socket = socket.clone();
        let job_id = props.job_id.clone();
        use_effect_with(props.job_id.clone(), move |_| {
            if let Some(existing) = socket.borrow_mut().take() {
                let _ = existing.socket.close();
            }

            events.set(Vec::new());
            error.set(None);
            connection.set("connecting".to_owned());

            let job = job.clone();
            let events = events.clone();
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
                    Ok(value) => events.set(value),
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
                            let message_error = error.clone();
                            let on_message = Closure::wrap(Box::new(move |event: MessageEvent| {
                                if let Some(text) = event.data().as_string() {
                                    match serde_json::from_str::<LogEvent>(&text) {
                                        Ok(log_event) => {
                                            let mut next = (*message_events).clone();
                                            if !next.iter().any(|existing| {
                                                existing.ts == log_event.ts
                                                    && existing.message == log_event.message
                                                    && existing.channel == log_event.channel
                                                    && existing.level == log_event.level
                                            }) {
                                                next.push(log_event);
                                                next.sort_by(|left, right| left.ts.cmp(&right.ts));
                                                message_events.set(next);
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
        "connected" => "status status-ok",
        "connecting" => "status status-warn",
        _ => "status status-muted",
    };

    html! {
        <section class="grid">
            <article class="card">
                <span class="eyebrow">{"Job"}</span>
                {
                    if let Some(message) = (*error).clone() {
                        html! { <p>{message}</p> }
                    } else if let Some(job) = (*job).clone() {
                        html! {
                            <>
                                <h2>{job.action}</h2>
                                <p class="mono muted">{job.id}</p>
                                <div class="log-meta">
                                    <span class={job_status_class(&job.status)}>{format!("{:?}", job.status)}</span>
                                    <span class={status_class}>{(*connection).clone()}</span>
                                </div>
                            </>
                        }
                    } else {
                        html! { <p>{"Loading job..."}</p> }
                    }
                }
            </article>
            <article class="card">
                <span class="eyebrow">{"Events"}</span>
                <div class="timeline">
                    {
                        if events.is_empty() {
                            html! { <p class="muted">{"Waiting for job events..."}</p> }
                        } else {
                            html! { for (*events).iter().rev().map(view_event) }
                        }
                    }
                </div>
            </article>
        </section>
    }
}

fn job_status_class(status: &JobStatus) -> &'static str {
    match status {
        JobStatus::Succeeded => "status status-ok",
        JobStatus::Failed => "status status-error",
        JobStatus::Cancelled => "status status-muted",
        JobStatus::Pending | JobStatus::Running => "status status-warn",
    }
}

fn view_event(event: &LogEvent) -> Html {
    let level_class = match event.level {
        LogLevel::Error => "status status-error",
        LogLevel::Warn => "status status-warn",
        LogLevel::Info => "status status-ok",
        LogLevel::Debug => "status status-muted",
    };

    html! {
        <div class="log-entry">
            <div class="log-entry-head">
                <span class={level_class}>{format!("{:?}", event.level)}</span>
                <span class="mono muted">{event.ts.to_rfc3339()}</span>
            </div>
            <p class="log-message mono">{event.message.clone()}</p>
            <div class="log-tags">
                <span class="status status-muted">{event.device_id.clone()}</span>
                <span class="status status-muted">{event.job_id.clone()}</span>
                <span class="status status-muted">{format_channel(&event.channel)}</span>
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

impl Drop for WsBindings {
    fn drop(&mut self) {
        let _ = self.socket.close();
        self.socket.set_onopen(None);
        self.socket.set_onmessage(None);
        self.socket.set_onerror(None);
        self.socket.set_onclose(None);
    }
}
