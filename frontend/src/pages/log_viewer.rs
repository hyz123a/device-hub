use device_hub_shared::{LogChannel, LogEvent, LogLevel};
use wasm_bindgen::{closure::Closure, JsCast};
use web_sys::{ErrorEvent, Event, HtmlInputElement, MessageEvent, WebSocket};
use yew::prelude::*;

use crate::api;

struct WsBindings {
    socket: WebSocket,
    _on_open: Closure<dyn FnMut(Event)>,
    _on_message: Closure<dyn FnMut(MessageEvent)>,
    _on_error: Closure<dyn FnMut(ErrorEvent)>,
    _on_close: Closure<dyn FnMut(Event)>,
}

#[function_component(LogViewerPage)]
pub fn log_viewer_page() -> Html {
    let events = use_state(Vec::<LogEvent>::new);
    let event_store = use_mut_ref(Vec::<LogEvent>::new);
    let device_filter = use_state(String::new);
    let job_filter = use_state(String::new);
    let connection = use_state(|| "connecting".to_owned());
    let error = use_state(|| Option::<String>::None);
    let connect_revision = use_state(|| 0_u32);
    let socket = use_mut_ref(|| Option::<WsBindings>::None);

    {
        let events = events.clone();
        let event_store = event_store.clone();
        let device_filter = device_filter.clone();
        let job_filter = job_filter.clone();
        let connection = connection.clone();
        let error = error.clone();
        let socket = socket.clone();
        use_effect_with(*connect_revision, move |_| {
            if let Some(existing) = socket.borrow_mut().take() {
                let _ = existing.socket.close();
            }

            connection.set("connecting".to_owned());
            error.set(None);
            events.set(Vec::new());
            *event_store.borrow_mut() = Vec::new();

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
                                            if next.len() > 200 {
                                                let drain = next.len() - 200;
                                                next.drain(0..drain);
                                            }
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
            move || {
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

    let status_class = match connection.as_str() {
        "connected" => "status status-ok",
        "connecting" => "status status-warn",
        _ => "status status-muted",
    };

    html! {
        <section class="grid">
            <article class="card" style="grid-column: 1 / -1;">
                <span class="eyebrow">{"Live logs"}</span>
                <h2>{"WebSocket event stream"}</h2>
                <p class="muted">{"Subscribe to all events or narrow the stream to one device or one job. Reconnect after changing filters to open a new `/api/ws` subscription."}</p>
                <p class="muted">{"Filters are combined: when both fields are set, events must match both device and job."}</p>
                <div class="log-toolbar">
                    <label class="field">
                        <span>{"Device filter"}</span>
                        <input value={(*device_filter).clone()} oninput={on_device_input} placeholder="board-alpha" />
                    </label>
                    <label class="field">
                        <span>{"Job filter"}</span>
                        <input value={(*job_filter).clone()} oninput={on_job_input} placeholder="job_123" />
                    </label>
                    <button class="button" onclick={on_reconnect}>{"Reconnect stream"}</button>
                </div>
                <div class="log-meta">
                    <span class={status_class}>{(*connection).clone()}</span>
                    <span class="muted mono">{format!("{} events buffered", events.len())}</span>
                </div>
                {
                    if let Some(message) = (*error).clone() {
                        html! { <p class="muted">{message}</p> }
                    } else {
                        Html::default()
                    }
                }
            </article>
            <article class="card" style="grid-column: 1 / -1;">
                <div class="timeline">
                    {
                        if events.is_empty() {
                            html! { <p class="muted">{"Waiting for live events..."}</p> }
                        } else {
                            html! { for (*events).iter().rev().map(view_event) }
                        }
                    }
                </div>
            </article>
        </section>
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
                <span class="status status-muted">{format!("{:?}", event.source)}</span>
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
