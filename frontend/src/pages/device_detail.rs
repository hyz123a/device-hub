use wasm_bindgen_futures::spawn_local;
use yew::prelude::*;
use yew_router::prelude::*;

use crate::{api, app::Route};
use device_hub_shared::{ActiveTailStatus, DeviceDetail, Job, JobStatus};

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
    let jlink_firmware_path = use_state(String::new);
    let pending_action = use_state(|| Option::<String>::None);
    let running_ssh_tail = use_state(|| Option::<ActiveTailStatus>::None);
    let running_serial_tail = use_state(|| Option::<ActiveTailStatus>::None);
    let running_adb_logcat = use_state(|| Option::<ActiveTailStatus>::None);
    let navigator = use_navigator();

    {
        let detail = detail.clone();
        let error = error.clone();
        let running_ssh_tail = running_ssh_tail.clone();
        let running_serial_tail = running_serial_tail.clone();
        let running_adb_logcat = running_adb_logcat.clone();
        let device_id = props.device_id.clone();
        use_effect_with(props.device_id.clone(), move |_| {
            spawn_local(async move {
                match api::get_device(&device_id).await {
                    Ok(value) => {
                        running_ssh_tail.set(find_active_tail(&value, "ssh.tail"));
                        running_serial_tail.set(find_active_tail(&value, "serial.tail"));
                        running_adb_logcat.set(find_active_tail(&value, "adb.logcat"));
                        detail.set(Some(value));
                    }
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
                                        <h3 class="mb-1 mt-0 text-lg font-semibold text-ink">{"Serial"}</h3>
                                        <p class="m-0 text-ink-muted">{if detail.capabilities.has_serial { "Configured" } else { "Unavailable" }}</p>
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
                        let default_ssh_log_path = detail
                            .device
                            .ssh
                            .as_ref()
                            .map(|ssh| ssh.default_log_path.clone())
                            .unwrap_or_default();
                        let pending_action_value = (*pending_action).clone();
                        let running_ssh_tail_value = (*running_ssh_tail).clone();
                        let device_logs_route = Route::DeviceLogs {
                            device_id: device_id.clone(),
                        };

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
                            let running_ssh_tail = running_ssh_tail.clone();
                            Callback::from(move |_| {
                                let device_id = device_id.clone();
                                let path = (*tail_path).trim().to_owned();
                                let payload = (!path.is_empty()).then_some(path);
                                let error = error.clone();
                                let pending_action = pending_action.clone();
                                let navigator = navigator.clone();
                                let running_ssh_tail = running_ssh_tail.clone();
                                let active_job = (*running_ssh_tail).clone();
                                let action_label = if active_job.is_some() {
                                    "ssh.tail.stop"
                                } else {
                                    "ssh.tail"
                                };
                                pending_action.set(Some(action_label.to_owned()));
                                spawn_local(async move {
                                    let result = if let Some(active_job) = active_job {
                                        api::cancel_job(&active_job.job_id).await
                                    } else {
                                        api::start_action(&device_id, "ssh.tail", payload).await
                                    };

                                    match result {
                                        Ok(job) => {
                                            pending_action.set(None);
                                            error.set(None);
                                            if job.status == JobStatus::Cancelled {
                                                running_ssh_tail.set(None);
                                            } else {
                                                running_ssh_tail.set(Some(active_tail_from_job(&job)));
                                                if let Some(navigator) = navigator {
                                                    navigator.push(&Route::JobDetail { job_id: job.id });
                                                }
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
                                <div class="mt-4 flex flex-wrap justify-end gap-3">
                                    <Link<Route>
                                        classes="rounded-2xl border border-black/10 bg-white/75 px-4 py-3 text-sm font-medium text-ink transition hover:bg-white"
                                        to={device_logs_route.clone()}
                                    >{"Open device logs"}</Link<Route>>
                                </div>
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
                                            placeholder={default_ssh_log_path.clone()}
                                        />
                                        <span>{format!("Leave empty to use the device default log path: {default_ssh_log_path}")}</span>
                                    </label>
                                    <div class="flex flex-wrap justify-end gap-3">
                                        {
                                            if let Some(job) = running_ssh_tail_value.clone() {
                                                let navigator = navigator.clone();
                                                let job_id = job.job_id.clone();
                                                let on_open_ssh_tail = Callback::from(move |_| {
                                                    if let Some(navigator) = navigator.clone() {
                                                        navigator.push(&Route::JobDetail { job_id: job_id.clone() });
                                                    }
                                                });

                                                html! {
                                                    <button
                                                        class="rounded-2xl border border-black/10 bg-white/75 px-4 py-3 font-medium text-ink transition hover:bg-white disabled:cursor-default disabled:opacity-55"
                                                        onclick={on_open_ssh_tail}
                                                        disabled={pending_action_value.is_some()}
                                                    >{"Open ssh.tail"}</button>
                                                }
                                            } else {
                                                Html::default()
                                            }
                                        }
                                        <button
                                            class="rounded-2xl bg-gradient-to-br from-accent to-accent-strong px-4 py-3 font-medium text-white transition hover:opacity-95 disabled:cursor-default disabled:opacity-55"
                                            onclick={on_tail}
                                            disabled={pending_action_value.is_some()}
                                        >{
                                            if running_ssh_tail_value.is_some() {
                                                "Stop ssh.tail"
                                            } else {
                                                "Start ssh.tail"
                                            }
                                        }</button>
                                    </div>
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
                                        if action == "ssh.tail" {
                                            html! { <p class="mb-0 mt-4 text-sm text-ink-muted">{"Starting ssh.tail..."}</p> }
                                        } else if action == "ssh.tail.stop" {
                                            html! { <p class="mb-0 mt-4 text-sm text-ink-muted">{"Stopping ssh.tail..."}</p> }
                                        } else {
                                            html! { <p class="mb-0 mt-4 text-sm text-ink-muted">{format!("Starting {action}...")}</p> }
                                        }
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
            {
                if let Some(detail) = (*detail).clone() {
                    if detail.capabilities.has_serial {
                        let device_id = detail.device.id.clone();
                        let pending_action_value = (*pending_action).clone();
                        let running_serial_tail_value = (*running_serial_tail).clone();
                        let device_logs_route = Route::DeviceLogs {
                            device_id: device_id.clone(),
                        };

                        let on_serial_tail = {
                            let device_id = device_id.clone();
                            let error = error.clone();
                            let pending_action = pending_action.clone();
                            let navigator = navigator.clone();
                            let running_serial_tail = running_serial_tail.clone();
                            Callback::from(move |_| {
                                let device_id = device_id.clone();
                                let error = error.clone();
                                let pending_action = pending_action.clone();
                                let navigator = navigator.clone();
                                let running_serial_tail = running_serial_tail.clone();
                                let active_job = (*running_serial_tail).clone();
                                let action_label = if active_job.is_some() {
                                    "serial.tail.stop"
                                } else {
                                    "serial.tail"
                                };
                                pending_action.set(Some(action_label.to_owned()));
                                spawn_local(async move {
                                    let result = if let Some(active_job) = active_job {
                                        api::cancel_job(&active_job.job_id).await
                                    } else {
                                        api::start_action(&device_id, "serial.tail", None).await
                                    };

                                    match result {
                                        Ok(job) => {
                                            pending_action.set(None);
                                            error.set(None);
                                            if job.status == JobStatus::Cancelled {
                                                running_serial_tail.set(None);
                                            } else {
                                                running_serial_tail.set(Some(active_tail_from_job(&job)));
                                                if let Some(navigator) = navigator {
                                                    navigator.push(&Route::JobDetail { job_id: job.id });
                                                }
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
                                <span class="inline-flex items-center gap-2 uppercase tracking-[0.18em] text-[0.74rem] text-accent-strong">{"Serial actions"}</span>
                                <div class="mt-4 flex flex-wrap justify-end gap-3">
                                    <Link<Route>
                                        classes="rounded-2xl border border-black/10 bg-white/75 px-4 py-3 text-sm font-medium text-ink transition hover:bg-white"
                                        to={device_logs_route}
                                    >{"Open device logs"}</Link<Route>>
                                </div>
                                <div class="mt-4 grid items-end gap-4 md:grid-cols-[minmax(0,1fr)_auto]">
                                    <div class="grid gap-2 text-sm text-ink-muted">
                                        <span>{"Tail device console"}</span>
                                        <p class="m-0">
                                            {
                                                if let Some(job) = running_serial_tail_value.clone() {
                                                    format!("Serial log streaming is active in {} since {}. Stop it here or open the job page.", job.job_id, job.active_since)
                                                } else {
                                                    "Open the configured serial port as a live log job and follow it on the job page.".to_owned()
                                                }
                                            }
                                        </p>
                                    </div>
                                    <div class="flex flex-wrap justify-end gap-3">
                                        {
                                            if let Some(job) = running_serial_tail_value.clone() {
                                                let navigator = navigator.clone();
                                                let job_id = job.job_id.clone();
                                                let on_open_serial_tail = Callback::from(move |_| {
                                                    if let Some(navigator) = navigator.clone() {
                                                        navigator.push(&Route::JobDetail { job_id: job_id.clone() });
                                                    }
                                                });

                                                html! {
                                                    <button
                                                        class="rounded-2xl border border-black/10 bg-white/75 px-4 py-3 font-medium text-ink transition hover:bg-white disabled:cursor-default disabled:opacity-55"
                                                        onclick={on_open_serial_tail}
                                                        disabled={pending_action_value.is_some()}
                                                    >{"Open serial.tail"}</button>
                                                }
                                            } else {
                                                Html::default()
                                            }
                                        }
                                        <button
                                            class="rounded-2xl bg-gradient-to-br from-accent to-accent-strong px-4 py-3 font-medium text-white transition hover:opacity-95 disabled:cursor-default disabled:opacity-55"
                                            onclick={on_serial_tail}
                                            disabled={pending_action_value.is_some()}
                                        >{
                                            if running_serial_tail_value.is_some() {
                                                "Stop serial.tail"
                                            } else {
                                                "Start serial.tail"
                                            }
                                        }</button>
                                    </div>
                                </div>
                                {
                                    if let Some(action) = pending_action_value {
                                        if action == "serial.tail" {
                                            html! { <p class="mb-0 mt-4 text-sm text-ink-muted">{"Starting serial.tail..."}</p> }
                                        } else if action == "serial.tail.stop" {
                                            html! { <p class="mb-0 mt-4 text-sm text-ink-muted">{"Stopping serial.tail..."}</p> }
                                        } else {
                                            Html::default()
                                        }
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
            {
                if let Some(detail) = (*detail).clone() {
                    if detail.capabilities.has_adb {
                        let device_id = detail.device.id.clone();
                        let pending_action_value = (*pending_action).clone();
                        let running_adb_logcat_value = (*running_adb_logcat).clone();
                        let device_logs_route = Route::DeviceLogs {
                            device_id: device_id.clone(),
                        };

                        let on_adb_logcat = {
                            let device_id = device_id.clone();
                            let error = error.clone();
                            let pending_action = pending_action.clone();
                            let navigator = navigator.clone();
                            let running_adb_logcat = running_adb_logcat.clone();
                            Callback::from(move |_| {
                                let device_id = device_id.clone();
                                let error = error.clone();
                                let pending_action = pending_action.clone();
                                let navigator = navigator.clone();
                                let running_adb_logcat = running_adb_logcat.clone();
                                let active_job = (*running_adb_logcat).clone();
                                let action_label = if active_job.is_some() {
                                    "adb.logcat.stop"
                                } else {
                                    "adb.logcat"
                                };
                                pending_action.set(Some(action_label.to_owned()));
                                spawn_local(async move {
                                    let result = if let Some(active_job) = active_job {
                                        api::cancel_job(&active_job.job_id).await
                                    } else {
                                        api::start_action(&device_id, "adb.logcat", None).await
                                    };

                                    match result {
                                        Ok(job) => {
                                            pending_action.set(None);
                                            error.set(None);
                                            if job.status == JobStatus::Cancelled {
                                                running_adb_logcat.set(None);
                                            } else {
                                                running_adb_logcat.set(Some(active_tail_from_job(&job)));
                                                if let Some(navigator) = navigator {
                                                    navigator.push(&Route::JobDetail { job_id: job.id });
                                                }
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
                                <span class="inline-flex items-center gap-2 uppercase tracking-[0.18em] text-[0.74rem] text-accent-strong">{"ADB actions"}</span>
                                <div class="mt-4 flex flex-wrap justify-end gap-3">
                                    <Link<Route>
                                        classes="rounded-2xl border border-black/10 bg-white/75 px-4 py-3 text-sm font-medium text-ink transition hover:bg-white"
                                        to={device_logs_route}
                                    >{"Open device logs"}</Link<Route>>
                                </div>
                                <div class="mt-4 grid items-end gap-4 md:grid-cols-[minmax(0,1fr)_auto]">
                                    <div class="grid gap-2 text-sm text-ink-muted">
                                        <span>{"Tail Android logcat"}</span>
                                        <p class="m-0">
                                            {
                                                if let Some(job) = running_adb_logcat_value.clone() {
                                                    format!("ADB logcat streaming is active in {} since {}. Stop it here or open the job page.", job.job_id, job.active_since)
                                                } else {
                                                    "Start adb.logcat as a live log job and follow it on the job page.".to_owned()
                                                }
                                            }
                                        </p>
                                    </div>
                                    <div class="flex flex-wrap justify-end gap-3">
                                        {
                                            if let Some(job) = running_adb_logcat_value.clone() {
                                                let navigator = navigator.clone();
                                                let job_id = job.job_id.clone();
                                                let on_open_adb_logcat = Callback::from(move |_| {
                                                    if let Some(navigator) = navigator.clone() {
                                                        navigator.push(&Route::JobDetail { job_id: job_id.clone() });
                                                    }
                                                });

                                                html! {
                                                    <button
                                                        class="rounded-2xl border border-black/10 bg-white/75 px-4 py-3 font-medium text-ink transition hover:bg-white disabled:cursor-default disabled:opacity-55"
                                                        onclick={on_open_adb_logcat}
                                                        disabled={pending_action_value.is_some()}
                                                    >{"Open adb.logcat"}</button>
                                                }
                                            } else {
                                                Html::default()
                                            }
                                        }
                                        <button
                                            class="rounded-2xl bg-gradient-to-br from-accent to-accent-strong px-4 py-3 font-medium text-white transition hover:opacity-95 disabled:cursor-default disabled:opacity-55"
                                            onclick={on_adb_logcat}
                                            disabled={pending_action_value.is_some()}
                                        >{
                                            if running_adb_logcat_value.is_some() {
                                                "Stop adb.logcat"
                                            } else {
                                                "Start adb.logcat"
                                            }
                                        }</button>
                                    </div>
                                </div>
                                {
                                    if let Some(action) = pending_action_value {
                                        if action == "adb.logcat" {
                                            html! { <p class="mb-0 mt-4 text-sm text-ink-muted">{"Starting adb.logcat..."}</p> }
                                        } else if action == "adb.logcat.stop" {
                                            html! { <p class="mb-0 mt-4 text-sm text-ink-muted">{"Stopping adb.logcat..."}</p> }
                                        } else {
                                            Html::default()
                                        }
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
            {
                if let Some(detail) = (*detail).clone() {
                    if detail.capabilities.has_jlink {
                        let device_id = detail.device.id.clone();
                        let pending_action_value = (*pending_action).clone();
                        let jlink_firmware_path_value = (*jlink_firmware_path).clone();

                        let on_jlink_firmware_input = {
                            let jlink_firmware_path = jlink_firmware_path.clone();
                            Callback::from(move |event: InputEvent| {
                                let input: web_sys::HtmlInputElement = event.target_unchecked_into();
                                jlink_firmware_path.set(input.value());
                            })
                        };

                        let on_jlink_probe = {
                            let device_id = device_id.clone();
                            let error = error.clone();
                            let pending_action = pending_action.clone();
                            let navigator = navigator.clone();
                            Callback::from(move |_| {
                                let device_id = device_id.clone();
                                let error = error.clone();
                                let pending_action = pending_action.clone();
                                let navigator = navigator.clone();
                                pending_action.set(Some("jlink.probe".to_owned()));
                                spawn_local(async move {
                                    match api::start_action(&device_id, "jlink.probe", None).await {
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

                        let on_jlink_flash = {
                            let device_id = device_id.clone();
                            let error = error.clone();
                            let pending_action = pending_action.clone();
                            let navigator = navigator.clone();
                            let jlink_firmware_path = jlink_firmware_path.clone();
                            Callback::from(move |_| {
                                let device_id = device_id.clone();
                                let firmware_path = (*jlink_firmware_path).trim().to_owned();
                                let error = error.clone();
                                let pending_action = pending_action.clone();
                                let navigator = navigator.clone();
                                pending_action.set(Some("jlink.flash".to_owned()));
                                spawn_local(async move {
                                    match api::start_action(&device_id, "jlink.flash", Some(firmware_path)).await {
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

                        let on_jlink_verify = {
                            let device_id = device_id.clone();
                            let error = error.clone();
                            let pending_action = pending_action.clone();
                            let navigator = navigator.clone();
                            Callback::from(move |_| {
                                let device_id = device_id.clone();
                                let error = error.clone();
                                let pending_action = pending_action.clone();
                                let navigator = navigator.clone();
                                pending_action.set(Some("jlink.verify".to_owned()));
                                spawn_local(async move {
                                    match api::start_action(&device_id, "jlink.verify", None).await {
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

                        let on_jlink_reset = {
                            let device_id = device_id.clone();
                            let error = error.clone();
                            let pending_action = pending_action.clone();
                            let navigator = navigator.clone();
                            Callback::from(move |_| {
                                let device_id = device_id.clone();
                                let error = error.clone();
                                let pending_action = pending_action.clone();
                                let navigator = navigator.clone();
                                pending_action.set(Some("jlink.reset".to_owned()));
                                spawn_local(async move {
                                    match api::start_action(&device_id, "jlink.reset", None).await {
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
                                <span class="inline-flex items-center gap-2 uppercase tracking-[0.18em] text-[0.74rem] text-accent-strong">{"J-Link actions"}</span>
                                <div class="mt-4 grid items-end gap-4 md:grid-cols-[minmax(0,1fr)_auto_auto]">
                                    <div class="grid gap-2 text-sm text-ink-muted">
                                        <span>{"Probe target"}</span>
                                        <p class="m-0">{"Query the configured probe and capture structured evidence for the connected target."}</p>
                                    </div>
                                    <button
                                        class="rounded-2xl bg-gradient-to-br from-accent to-accent-strong px-4 py-3 font-medium text-white transition hover:opacity-95 disabled:cursor-default disabled:opacity-55 md:col-start-3"
                                        onclick={on_jlink_probe}
                                        disabled={pending_action_value.is_some()}
                                    >{"Run jlink.probe"}</button>
                                    <label class="grid gap-2 text-sm text-ink-muted md:col-span-2">
                                        <span>{"Firmware path"}</span>
                                        <input
                                            class="w-full rounded-2xl border border-black/10 bg-white/75 px-4 py-3 text-ink outline-none transition placeholder:text-ink-muted/70 focus:border-accent/40 focus:ring-2 focus:ring-accent/10"
                                            value={jlink_firmware_path_value}
                                            oninput={on_jlink_firmware_input}
                                            placeholder="/tmp/firmware.bin"
                                        />
                                    </label>
                                    <button
                                        class="rounded-2xl bg-gradient-to-br from-accent to-accent-strong px-4 py-3 font-medium text-white transition hover:opacity-95 disabled:cursor-default disabled:opacity-55"
                                        onclick={on_jlink_flash}
                                        disabled={pending_action_value.is_some()}
                                    >{"Run jlink.flash"}</button>
                                    <div class="grid gap-2 text-sm text-ink-muted">
                                        <span>{"Verify target memory"}</span>
                                        <p class="m-0">{"Run a verification pass and store the tool transcript plus structured result payload."}</p>
                                    </div>
                                    <button
                                        class="rounded-2xl bg-gradient-to-br from-accent to-accent-strong px-4 py-3 font-medium text-white transition hover:opacity-95 disabled:cursor-default disabled:opacity-55 md:col-start-3"
                                        onclick={on_jlink_verify}
                                        disabled={pending_action_value.is_some()}
                                    >{"Run jlink.verify"}</button>
                                    <div class="grid gap-2 text-sm text-ink-muted">
                                        <span>{"Reset target"}</span>
                                        <p class="m-0">{"Request a target reset through the configured J-Link probe."}</p>
                                    </div>
                                    <button
                                        class="rounded-2xl bg-gradient-to-br from-accent to-accent-strong px-4 py-3 font-medium text-white transition hover:opacity-95 disabled:cursor-default disabled:opacity-55 md:col-start-3"
                                        onclick={on_jlink_reset}
                                        disabled={pending_action_value.is_some()}
                                    >{"Run jlink.reset"}</button>
                                </div>
                                {
                                    if let Some(action) = pending_action_value {
                                        if action.starts_with("jlink.") {
                                            html! { <p class="mb-0 mt-4 text-sm text-ink-muted">{format!("Starting {action}...")}</p> }
                                        } else {
                                            Html::default()
                                        }
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

fn find_active_tail(detail: &DeviceDetail, action: &str) -> Option<ActiveTailStatus> {
    detail
        .runtime
        .active_tails
        .iter()
        .find(|tail| tail.action == action)
        .cloned()
}

fn active_tail_from_job(job: &Job) -> ActiveTailStatus {
    ActiveTailStatus {
        action: job.action.clone(),
        job_id: job.id.clone(),
        active_since: job.created_at,
    }
}
