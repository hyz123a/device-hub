use device_hub_shared::LogEvent;
use wasm_bindgen::{closure::Closure, JsCast};
use web_sys::{ErrorEvent, Event, HtmlInputElement, MessageEvent, WebSocket};
use yew::prelude::*;

use crate::api;
use crate::components::log_event::LogEventCard;

struct WsBindings {
    socket: WebSocket,
    _on_open: Closure<dyn FnMut(Event)>,
    _on_message: Closure<dyn FnMut(MessageEvent)>,
    _on_error: Closure<dyn FnMut(ErrorEvent)>,
    _on_close: Closure<dyn FnMut(Event)>,
}

const EVENT_PAGE_SIZE: usize = 50;

#[function_component(LogViewerPage)]
pub fn log_viewer_page() -> Html {
    let events = use_state(Vec::<LogEvent>::new);
    let current_page = use_state(|| 0_usize);
    let event_store = use_mut_ref(Vec::<LogEvent>::new);
    let device_filter = use_state(String::new);
    let job_filter = use_state(String::new);
    let connection = use_state(|| "connecting".to_owned());
    let error = use_state(|| Option::<String>::None);
    let connect_revision = use_state(|| 0_u32);
    let socket = use_mut_ref(|| Option::<WsBindings>::None);

    {
        let events = events.clone();
        let current_page = current_page.clone();
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
            current_page.set(0);
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
            <article class="rounded-[24px] border border-black/10 bg-white/80 p-6 shadow-shell backdrop-blur xl:col-span-2">
                <span class="inline-flex items-center gap-2 uppercase tracking-[0.18em] text-[0.74rem] text-accent-strong">{"Live logs"}</span>
                <h2 class="mb-3 mt-3 text-3xl font-semibold tracking-tight text-ink">{"WebSocket event stream"}</h2>
                <p class="m-0 text-ink-muted">{"Subscribe to all events or narrow the stream to one device or one job. Reconnect after changing filters to open a new `/api/ws` subscription."}</p>
                <p class="mb-0 mt-2 text-ink-muted">{"Filters are combined: when both fields are set, events must match both device and job."}</p>
                <div class="mt-5 grid items-end gap-4 lg:grid-cols-3">
                    <label class="grid gap-2 text-sm text-ink-muted">
                        <span>{"Device filter"}</span>
                        <input class="w-full rounded-2xl border border-black/10 bg-white/75 px-4 py-3 text-ink outline-none transition placeholder:text-ink-muted/70 focus:border-accent/40 focus:ring-2 focus:ring-accent/10" value={(*device_filter).clone()} oninput={on_device_input} placeholder="board-alpha" />
                    </label>
                    <label class="grid gap-2 text-sm text-ink-muted">
                        <span>{"Job filter"}</span>
                        <input class="w-full rounded-2xl border border-black/10 bg-white/75 px-4 py-3 text-ink outline-none transition placeholder:text-ink-muted/70 focus:border-accent/40 focus:ring-2 focus:ring-accent/10" value={(*job_filter).clone()} oninput={on_job_input} placeholder="job_123" />
                    </label>
                    <button class="rounded-2xl bg-gradient-to-br from-accent to-accent-strong px-4 py-3 font-medium text-white transition hover:opacity-95" onclick={on_reconnect}>{"Reconnect stream"}</button>
                </div>
                <div class="mt-4 flex flex-wrap items-center gap-3">
                    <span class={status_class}>{(*connection).clone()}</span>
                    <span class="font-mono text-sm text-ink-muted">{format!("{} events buffered", events.len())}</span>
                </div>
                {
                    if let Some(message) = (*error).clone() {
                        html! { <p class="mb-0 mt-4 text-sm text-red-700">{message}</p> }
                    } else {
                        Html::default()
                    }
                }
            </article>
            <article class="rounded-[24px] border border-black/10 bg-white/80 p-6 shadow-shell backdrop-blur xl:col-span-2">
                <div class="mb-5 flex flex-wrap items-center justify-between gap-3">
                    <span class="font-mono text-sm text-ink-muted">{format!("page {} / {} · showing {} of {}", page + 1, total_pages, shown_events, total_events)}</span>
                    <div class="flex flex-wrap gap-2.5">
                        <button class="rounded-2xl border border-black/10 bg-white/75 px-4 py-3 font-medium text-ink transition hover:bg-white disabled:cursor-default disabled:opacity-55" onclick={on_show_newer} disabled={page == 0}>{"Newer"}</button>
                        <button class="rounded-2xl border border-black/10 bg-white/75 px-4 py-3 font-medium text-ink transition hover:bg-white disabled:cursor-default disabled:opacity-55" onclick={on_show_older} disabled={start_index == 0 || total_events == 0}>{"Older"}</button>
                    </div>
                </div>
                <div class="grid gap-4">
                    {
                        if events.is_empty() {
                            html! { <p class="m-0 text-ink-muted">{"Waiting for live events..."}</p> }
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

impl Drop for WsBindings {
    fn drop(&mut self) {
        let _ = self.socket.close();
        self.socket.set_onopen(None);
        self.socket.set_onmessage(None);
        self.socket.set_onerror(None);
        self.socket.set_onclose(None);
    }
}
