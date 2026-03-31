use wasm_bindgen_futures::spawn_local;
use yew::prelude::*;

use crate::api;

#[derive(Properties, PartialEq)]
pub struct SerialWritePanelProps {
    pub device_id: String,
}

#[function_component(SerialWritePanel)]
pub fn serial_write_panel(props: &SerialWritePanelProps) -> Html {
    let serial_write_input = use_state(String::new);
    let pending_action = use_state(|| false);
    let error = use_state(|| Option::<String>::None);

    let on_serial_write_input = {
        let serial_write_input = serial_write_input.clone();
        Callback::from(move |event: InputEvent| {
            let input: web_sys::HtmlTextAreaElement = event.target_unchecked_into();
            serial_write_input.set(input.value());
        })
    };

    let on_serial_write = make_serial_write_action(
        props.device_id.clone(),
        serial_write_input.clone(),
        error.clone(),
        pending_action.clone(),
        false,
    );
    let on_serial_write_line = make_serial_write_action(
        props.device_id.clone(),
        serial_write_input.clone(),
        error.clone(),
        pending_action.clone(),
        true,
    );
    let on_serial_write_line_shortcut = {
        let device_id = props.device_id.clone();
        let serial_write_input = serial_write_input.clone();
        let error = error.clone();
        let pending_action = pending_action.clone();
        let serial_write_input = serial_write_input.clone();
        Callback::from(move |event: KeyboardEvent| {
            if event.ctrl_key() && event.key() == "Enter" && !*pending_action {
                event.prevent_default();
                trigger_serial_write(
                    device_id.clone(),
                    Some(format!("{}\n", *serial_write_input)),
                    Some(serial_write_input.clone()),
                    error.clone(),
                    pending_action.clone(),
                );
            }
        })
    };
    let on_serial_ctrl_c = make_serial_control_action(
        props.device_id.clone(),
        error.clone(),
        pending_action.clone(),
        '\u{3}',
    );
    let on_serial_ctrl_d = make_serial_control_action(
        props.device_id.clone(),
        error.clone(),
        pending_action.clone(),
        '\u{4}',
    );
    let on_serial_escape = make_serial_control_action(
        props.device_id.clone(),
        error.clone(),
        pending_action.clone(),
        '\u{1b}',
    );

    html! {
        <article class="mt-2 overflow-hidden rounded-[18px] border border-white/5 bg-[#0f172a]/70 shadow-inner ring-1 ring-inset ring-white/5 backdrop-blur-sm">
            <div class="grid gap-4 px-5 py-5">
            <label class="grid gap-2 text-sm text-slate-300">
                <div class="flex flex-wrap items-center justify-between gap-2">
                    <span>{"Serial input"}</span>
                    <span class="font-mono text-xs text-slate-500">{"Ctrl+Enter = Send line"}</span>
                </div>
                <textarea
                    class="min-h-28 w-full rounded-2xl border border-white/8 bg-black/25 px-4 py-3 font-mono text-sm text-slate-100 outline-none transition placeholder:text-slate-600 focus:border-accent/40 focus:ring-2 focus:ring-accent/10"
                    value={(*serial_write_input).clone()}
                    oninput={on_serial_write_input}
                    onkeydown={on_serial_write_line_shortcut}
                    placeholder="help"
                />
            </label>
            <div class="grid items-end gap-4 md:grid-cols-[minmax(0,1fr)_auto]">
                <div class="grid gap-2 text-sm text-slate-500">
                    <span>{"Actions"}</span>
                    <p class="m-0">{"Send raw text directly, append a newline for shell-style commands, or inject common control characters."}</p>
                </div>
                <div class="flex flex-wrap justify-end gap-2.5">
                    <button
                        class="rounded-2xl border border-white/8 bg-white/[0.04] px-3.5 py-2.5 font-medium text-slate-100 transition hover:bg-white/10 disabled:cursor-default disabled:opacity-55"
                        onclick={on_serial_ctrl_c}
                        disabled={*pending_action}
                    >{"Send Ctrl-C"}</button>
                    <button
                        class="rounded-2xl border border-white/8 bg-white/[0.04] px-3.5 py-2.5 font-medium text-slate-100 transition hover:bg-white/10 disabled:cursor-default disabled:opacity-55"
                        onclick={on_serial_ctrl_d}
                        disabled={*pending_action}
                    >{"Send Ctrl-D"}</button>
                    <button
                        class="rounded-2xl border border-white/8 bg-white/[0.04] px-3.5 py-2.5 font-medium text-slate-100 transition hover:bg-white/10 disabled:cursor-default disabled:opacity-55"
                        onclick={on_serial_escape}
                        disabled={*pending_action}
                    >{"Send Esc"}</button>
                    <button
                        class="rounded-2xl border border-white/8 bg-white/[0.04] px-3.5 py-2.5 font-medium text-slate-100 transition hover:bg-white/10 disabled:cursor-default disabled:opacity-55"
                        onclick={on_serial_write}
                        disabled={*pending_action}
                    >{"Send raw"}</button>
                    <button
                        class="rounded-2xl bg-gradient-to-br from-accent to-accent-strong px-3.5 py-2.5 font-medium text-white transition hover:opacity-95 disabled:cursor-default disabled:opacity-55"
                        onclick={on_serial_write_line}
                        disabled={*pending_action}
                    >{"Send line"}</button>
                </div>
            </div>
            {
                if *pending_action {
                    html! { <p class="mb-0 text-sm text-slate-400">{"Starting serial.write..."}</p> }
                } else if let Some(message) = (*error).clone() {
                    html! { <p class="mb-0 text-sm text-red-400">{message}</p> }
                } else {
                    Html::default()
                }
            }
            </div>
        </article>
    }
}

fn make_serial_write_action(
    device_id: String,
    serial_write_input: UseStateHandle<String>,
    error: UseStateHandle<Option<String>>,
    pending_action: UseStateHandle<bool>,
    append_newline: bool,
) -> Callback<MouseEvent> {
    Callback::from(move |_| {
        let payload = if append_newline {
            format!("{}\n", *serial_write_input)
        } else {
            (*serial_write_input).clone()
        };
        trigger_serial_write(
            device_id.clone(),
            Some(payload),
            Some(serial_write_input.clone()),
            error.clone(),
            pending_action.clone(),
        );
    })
}

fn make_serial_control_action(
    device_id: String,
    error: UseStateHandle<Option<String>>,
    pending_action: UseStateHandle<bool>,
    control: char,
) -> Callback<MouseEvent> {
    Callback::from(move |_| {
        trigger_serial_write(
            device_id.clone(),
            Some(control.to_string()),
            None,
            error.clone(),
            pending_action.clone(),
        );
    })
}

fn trigger_serial_write(
    device_id: String,
    payload: Option<String>,
    serial_write_input: Option<UseStateHandle<String>>,
    error: UseStateHandle<Option<String>>,
    pending_action: UseStateHandle<bool>,
) {
    pending_action.set(true);
    spawn_local(async move {
        match api::start_action(&device_id, "serial.write", payload).await {
            Ok(_) => {
                pending_action.set(false);
                error.set(None);
                if let Some(serial_write_input) = serial_write_input {
                    serial_write_input.set(String::new());
                }
            }
            Err(message) => {
                pending_action.set(false);
                error.set(Some(message));
            }
        }
    });
}
