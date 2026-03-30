use wasm_bindgen_futures::spawn_local;
use yew::prelude::*;
use yew_router::prelude::*;

use crate::{api, app::Route};
use device_hub_shared::DeviceDetail;

#[derive(Properties, PartialEq)]
pub struct DeviceDetailProps {
    pub device_id: String,
}

#[function_component(DeviceDetailPage)]
pub fn device_detail_page(props: &DeviceDetailProps) -> Html {
    let detail = use_state(|| Option::<DeviceDetail>::None);
    let error = use_state(|| Option::<String>::None);
    let exec_command = use_state(|| "uname -a".to_owned());
    let tail_path = use_state(String::new);
    let pending_action = use_state(|| Option::<String>::None);
    let navigator = use_navigator();

    {
        let detail = detail.clone();
        let error = error.clone();
        let device_id = props.device_id.clone();
        use_effect_with(props.device_id.clone(), move |_| {
            spawn_local(async move {
                match api::get_device(&device_id).await {
                    Ok(value) => detail.set(Some(value)),
                    Err(err) => error.set(Some(err)),
                }
            });
            || {}
        });
    }

    html! {
        <section class="grid gap-5 xl:grid-cols-2">
            <article class="rounded-[24px] border border-black/10 bg-white/80 p-6 shadow-shell backdrop-blur xl:col-span-2">
                <span class="inline-flex items-center gap-2 uppercase tracking-[0.18em] text-[0.74rem] text-accent-strong">{"Device detail"}</span>
                {
                    if let Some(message) = (*error).clone() {
                        html! { <p class="mb-0 mt-4 text-sm text-red-700">{message}</p> }
                    } else if let Some(detail) = (*detail).clone() {
                        html! {
                            <>
                                <h2 class="mb-2 mt-3 text-3xl font-semibold tracking-tight text-ink">{detail.device.label}</h2>
                                <p class="m-0 font-mono text-sm text-ink-muted">{detail.device.id}</p>
                                <div class="mt-5 flex flex-col gap-4 border-t border-black/10 pt-4 sm:flex-row sm:items-center sm:justify-between">
                                    <div>
                                        <h3 class="mb-1 mt-0 text-lg font-semibold text-ink">{"SSH"}</h3>
                                        <p class="m-0 text-ink-muted">{if detail.capabilities.has_ssh { "Configured" } else { "Unavailable" }}</p>
                                    </div>
                                    <div>
                                        <h3 class="mb-1 mt-0 text-lg font-semibold text-ink">{"J-Link"}</h3>
                                        <p class="m-0 text-ink-muted">{if detail.capabilities.has_jlink { "Configured" } else { "Unavailable" }}</p>
                                    </div>
                                </div>
                            </>
                        }
                    } else {
                        html! { <p class="mb-0 mt-4 text-ink-muted">{"Loading device..."}</p> }
                    }
                }
            </article>
            {
                if let Some(detail) = (*detail).clone() {
                    if detail.capabilities.has_ssh {
                        let exec_command_value = (*exec_command).clone();
                        let tail_path_value = (*tail_path).clone();
                        let device_id = detail.device.id.clone();
                        let pending_action_value = (*pending_action).clone();

                        let on_exec_input = {
                            let exec_command = exec_command.clone();
                            Callback::from(move |event: InputEvent| {
                                let input: web_sys::HtmlInputElement = event.target_unchecked_into();
                                exec_command.set(input.value());
                            })
                        };

                        let on_tail_input = {
                            let tail_path = tail_path.clone();
                            Callback::from(move |event: InputEvent| {
                                let input: web_sys::HtmlInputElement = event.target_unchecked_into();
                                tail_path.set(input.value());
                            })
                        };

                        let on_exec = {
                            let device_id = device_id.clone();
                            let exec_command = exec_command.clone();
                            let error = error.clone();
                            let pending_action = pending_action.clone();
                            let navigator = navigator.clone();
                            Callback::from(move |_| {
                                let device_id = device_id.clone();
                                let command = (*exec_command).trim().to_owned();
                                let error = error.clone();
                                let pending_action = pending_action.clone();
                                let navigator = navigator.clone();
                                pending_action.set(Some("ssh.exec".to_owned()));
                                spawn_local(async move {
                                    match api::start_action(&device_id, "ssh.exec", Some(command)).await {
                                        Ok(job) => {
                                            pending_action.set(None);
                                            error.set(None);
                                            if let Some(navigator) = navigator {
                                                navigator.push(&Route::JobDetail { job_id: job.id });
                                            }
                                        }
                                        Err(message) => {
                                            pending_action.set(None);
                                            error.set(Some(message));
                                        }
                                    }
                                });
                            })
                        };

                        let on_tail = {
                            let device_id = device_id.clone();
                            let tail_path = tail_path.clone();
                            let error = error.clone();
                            let pending_action = pending_action.clone();
                            let navigator = navigator.clone();
                            Callback::from(move |_| {
                                let device_id = device_id.clone();
                                let path = (*tail_path).trim().to_owned();
                                let error = error.clone();
                                let pending_action = pending_action.clone();
                                let navigator = navigator.clone();
                                pending_action.set(Some("ssh.tail".to_owned()));
                                spawn_local(async move {
                                    match api::start_action(&device_id, "ssh.tail", Some(path)).await {
                                        Ok(job) => {
                                            pending_action.set(None);
                                            error.set(None);
                                            if let Some(navigator) = navigator {
                                                navigator.push(&Route::JobDetail { job_id: job.id });
                                            }
                                        }
                                        Err(message) => {
                                            pending_action.set(None);
                                            error.set(Some(message));
                                        }
                                    }
                                });
                            })
                        };

                        let on_reboot = {
                            let device_id = device_id.clone();
                            let error = error.clone();
                            let pending_action = pending_action.clone();
                            let navigator = navigator.clone();
                            Callback::from(move |_| {
                                let device_id = device_id.clone();
                                let error = error.clone();
                                let pending_action = pending_action.clone();
                                let navigator = navigator.clone();
                                pending_action.set(Some("ssh.reboot".to_owned()));
                                spawn_local(async move {
                                    match api::start_action(&device_id, "ssh.reboot", None).await {
                                        Ok(job) => {
                                            pending_action.set(None);
                                            error.set(None);
                                            if let Some(navigator) = navigator {
                                                navigator.push(&Route::JobDetail { job_id: job.id });
                                            }
                                        }
                                        Err(message) => {
                                            pending_action.set(None);
                                            error.set(Some(message));
                                        }
                                    }
                                });
                            })
                        };

                        html! {
                            <article class="rounded-[24px] border border-black/10 bg-white/80 p-6 shadow-shell backdrop-blur xl:col-span-2">
                                <span class="inline-flex items-center gap-2 uppercase tracking-[0.18em] text-[0.74rem] text-accent-strong">{"SSH actions"}</span>
                                <div class="mt-4 grid items-end gap-4 md:grid-cols-[minmax(0,1fr)_auto]">
                                    <label class="grid gap-2 text-sm text-ink-muted">
                                        <span>{"Command"}</span>
                                        <input class="w-full rounded-2xl border border-black/10 bg-white/75 px-4 py-3 text-ink outline-none transition placeholder:text-ink-muted/70 focus:border-accent/40 focus:ring-2 focus:ring-accent/10" value={exec_command_value} oninput={on_exec_input} />
                                    </label>
                                    <button
                                        class="rounded-2xl bg-gradient-to-br from-accent to-accent-strong px-4 py-3 font-medium text-white transition hover:opacity-95 disabled:cursor-default disabled:opacity-55"
                                        onclick={on_exec}
                                        disabled={pending_action_value.is_some()}
                                    >{"Run ssh.exec"}</button>
                                    <label class="grid gap-2 text-sm text-ink-muted">
                                        <span>{"Log path"}</span>
                                        <input
                                            class="w-full rounded-2xl border border-black/10 bg-white/75 px-4 py-3 text-ink outline-none transition placeholder:text-ink-muted/70 focus:border-accent/40 focus:ring-2 focus:ring-accent/10"
                                            value={tail_path_value}
                                            oninput={on_tail_input}
                                            placeholder="/tmp/sdcard/bstationd.log"
                                        />
                                    </label>
                                    <button
                                        class="rounded-2xl bg-gradient-to-br from-accent to-accent-strong px-4 py-3 font-medium text-white transition hover:opacity-95 disabled:cursor-default disabled:opacity-55"
                                        onclick={on_tail}
                                        disabled={pending_action_value.is_some()}
                                    >{"Start ssh.tail"}</button>
                                    <div class="grid gap-2 text-sm text-ink-muted">
                                        <span>{"Reboot"}</span>
                                        <p class="m-0">{"Create a reboot job and follow it on the job page."}</p>
                                    </div>
                                    <button
                                        class="rounded-2xl bg-gradient-to-br from-accent to-accent-strong px-4 py-3 font-medium text-white transition hover:opacity-95 disabled:cursor-default disabled:opacity-55"
                                        onclick={on_reboot}
                                        disabled={pending_action_value.is_some()}
                                    >{"Run ssh.reboot"}</button>
                                </div>
                                {
                                    if let Some(action) = pending_action_value {
                                        html! { <p class="mb-0 mt-4 text-sm text-ink-muted">{format!("Starting {action}...")}</p> }
                                    } else {
                                        Html::default()
                                    }
                                }
                            </article>
                        }
                    } else {
                        Html::default()
                    }
                } else {
                    Html::default()
                }
            }
        </section>
    }
}
