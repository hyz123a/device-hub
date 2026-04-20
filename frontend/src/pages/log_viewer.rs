use web_sys::HtmlInputElement;
use yew::prelude::*;

use crate::api;
use crate::components::stream_log_panel::StreamLogPanel;

#[derive(Properties, PartialEq)]
pub struct LogViewerProps {
    #[prop_or_default]
    pub device_id: Option<String>,
}

#[function_component(LogViewerPage)]
pub fn log_viewer_page(props: &LogViewerProps) -> Html {
    let device_filter = use_state(|| props.device_id.clone().unwrap_or_default());
    let job_filter = use_state(String::new);
    let connect_revision = use_state(|| 0_u32);
    let connection = use_state(|| "connecting".to_owned());
    let error = use_state(|| Option::<String>::None);

    {
        let device_filter = device_filter.clone();
        let initial = props.device_id.clone();
        use_effect_with(props.device_id.clone(), move |_| {
            device_filter.set(initial.unwrap_or_default());
            || {}
        });
    }

    let ws_url = api::ws_url(Some(&device_filter), Some(&job_filter)).ok();

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
    let on_connection_change = {
        let connection = connection.clone();
        Callback::from(move |status: String| connection.set(status))
    };
    let on_error = {
        let error = error.clone();
        Callback::from(move |msg: String| error.set(Some(msg)))
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
                <div class="mt-4 flex flex-wrap items-center gap-3">
                    <span class={status_class}>{(*connection).clone()}</span>
                </div>
                {
                    if let Some(message) = (*error).clone() {
                        html! { <p class="mb-0 mt-4 text-sm text-red-700">{message}</p> }
                    } else {
                        Html::default()
                    }
                }
            </article>
            <div class="xl:col-span-2">
                <StreamLogPanel
                    ws_url={ws_url}
                    connect_revision={*connect_revision}
                    serial_write_device_id={props.device_id.clone()}
                    on_connection_change={on_connection_change}
                    on_error={on_error}
                />
            </div>
        </section>
    }
}
