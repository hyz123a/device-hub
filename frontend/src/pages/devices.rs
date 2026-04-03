use wasm_bindgen_futures::spawn_local;
use web_sys::HtmlInputElement;
use yew::prelude::*;
use yew_router::prelude::Link;

use crate::{api, app::Route};
use device_hub_shared::{Device, JlinkConfig, SerialConfig, SshConfig};

#[derive(Clone, PartialEq)]
struct DeviceDraft {
    id: String,
    label: String,
    ssh_enabled: bool,
    ssh_host: String,
    ssh_user: String,
    ssh_key: String,
    serial_enabled: bool,
    serial_port: String,
    serial_baud: String,
    jlink_enabled: bool,
    jlink_serial: String,
    jlink_device: String,
    jlink_interface: String,
    jlink_speed_khz: String,
    jlink_autoconnect: bool,
}

impl Default for DeviceDraft {
    fn default() -> Self {
        Self {
            id: String::new(),
            label: String::new(),
            ssh_enabled: false,
            ssh_host: String::new(),
            ssh_user: String::new(),
            ssh_key: String::new(),
            serial_enabled: false,
            serial_port: String::new(),
            serial_baud: "115200".into(),
            jlink_enabled: false,
            jlink_serial: String::new(),
            jlink_device: String::new(),
            jlink_interface: "SWD".into(),
            jlink_speed_khz: "4000".into(),
            jlink_autoconnect: true,
        }
    }
}

impl DeviceDraft {
    fn from_device(device: &Device) -> Self {
        let mut draft = Self {
            id: device.id.clone(),
            label: device.label.clone(),
            ..Self::default()
        };

        if let Some(ssh) = &device.ssh {
            draft.ssh_enabled = true;
            draft.ssh_host = ssh.host.clone();
            draft.ssh_user = ssh.user.clone();
            draft.ssh_key = ssh.key.clone();
        }
        if let Some(serial) = &device.serial {
            draft.serial_enabled = true;
            draft.serial_port = serial.port.clone();
            draft.serial_baud = serial.baud.to_string();
        }
        if let Some(jlink) = &device.jlink {
            draft.jlink_enabled = true;
            draft.jlink_serial = jlink.serial.clone();
            draft.jlink_device = jlink.device.clone();
            draft.jlink_interface = jlink.interface.clone();
            draft.jlink_speed_khz = jlink.speed_khz.to_string();
            draft.jlink_autoconnect = jlink.autoconnect;
        }

        draft
    }

    fn to_device(&self) -> Result<Device, String> {
        let id = self.id.trim().to_owned();
        if id.is_empty() {
            return Err("Device id is required".into());
        }
        let label = self.label.trim().to_owned();
        if label.is_empty() {
            return Err("Device label is required".into());
        }
        if !self.ssh_enabled && !self.serial_enabled && !self.jlink_enabled {
            return Err("Enable at least one connection before saving".into());
        }

        let ssh = if self.ssh_enabled {
            let host = self.ssh_host.trim().to_owned();
            let user = self.ssh_user.trim().to_owned();
            let key = self.ssh_key.trim().to_owned();
            if host.is_empty() || user.is_empty() || key.is_empty() {
                return Err("SSH config requires host, user, and key".into());
            }
            Some(SshConfig { host, user, key })
        } else {
            None
        };

        let serial = if self.serial_enabled {
            let port = self.serial_port.trim().to_owned();
            if port.is_empty() {
                return Err("Serial config requires a port".into());
            }
            let baud = self
                .serial_baud
                .trim()
                .parse::<u32>()
                .map_err(|_| "Serial baud must be a positive integer".to_owned())?;
            if baud == 0 {
                return Err("Serial baud must be greater than zero".into());
            }
            Some(SerialConfig { port, baud })
        } else {
            None
        };

        let jlink = if self.jlink_enabled {
            let serial = self.jlink_serial.trim().to_owned();
            let device = self.jlink_device.trim().to_owned();
            let interface = self.jlink_interface.trim().to_owned();
            if serial.is_empty() || device.is_empty() || interface.is_empty() {
                return Err("J-Link config requires serial, device, and interface".into());
            }
            let speed_khz = self
                .jlink_speed_khz
                .trim()
                .parse::<u32>()
                .map_err(|_| "J-Link speed must be a positive integer".to_owned())?;
            if speed_khz == 0 {
                return Err("J-Link speed must be greater than zero".into());
            }
            Some(JlinkConfig {
                serial,
                device,
                interface,
                speed_khz,
                autoconnect: self.jlink_autoconnect,
            })
        } else {
            None
        };

        Ok(Device {
            id,
            label,
            ssh,
            serial,
            jlink,
        })
    }
}

#[function_component(DevicesPage)]
pub fn devices_page() -> Html {
    let devices = use_state(|| Option::<Vec<Device>>::None);
    let page_error = use_state(|| Option::<String>::None);
    let form_error = use_state(|| Option::<String>::None);
    let notice = use_state(|| Option::<String>::None);
    let draft = use_state(DeviceDraft::default);
    let editing_device_id = use_state(|| Option::<String>::None);
    let submitting = use_state(|| false);

    {
        let devices = devices.clone();
        let page_error = page_error.clone();
        use_effect_with((), move |_| {
            refresh_devices(devices.clone(), page_error.clone());
            || {}
        });
    }

    let on_id_input = {
        let draft = draft.clone();
        Callback::from(move |event: InputEvent| {
            let value = event.target_unchecked_into::<HtmlInputElement>().value();
            update_draft(&draft, move |next| next.id = value);
        })
    };
    let on_label_input = {
        let draft = draft.clone();
        Callback::from(move |event: InputEvent| {
            let value = event.target_unchecked_into::<HtmlInputElement>().value();
            update_draft(&draft, move |next| next.label = value);
        })
    };
    let on_ssh_enabled = {
        let draft = draft.clone();
        Callback::from(move |event: Event| {
            let checked = event.target_unchecked_into::<HtmlInputElement>().checked();
            update_draft(&draft, move |next| next.ssh_enabled = checked);
        })
    };
    let on_ssh_host_input = {
        let draft = draft.clone();
        Callback::from(move |event: InputEvent| {
            let value = event.target_unchecked_into::<HtmlInputElement>().value();
            update_draft(&draft, move |next| next.ssh_host = value);
        })
    };
    let on_ssh_user_input = {
        let draft = draft.clone();
        Callback::from(move |event: InputEvent| {
            let value = event.target_unchecked_into::<HtmlInputElement>().value();
            update_draft(&draft, move |next| next.ssh_user = value);
        })
    };
    let on_ssh_key_input = {
        let draft = draft.clone();
        Callback::from(move |event: InputEvent| {
            let value = event.target_unchecked_into::<HtmlInputElement>().value();
            update_draft(&draft, move |next| next.ssh_key = value);
        })
    };
    let on_serial_enabled = {
        let draft = draft.clone();
        Callback::from(move |event: Event| {
            let checked = event.target_unchecked_into::<HtmlInputElement>().checked();
            update_draft(&draft, move |next| next.serial_enabled = checked);
        })
    };
    let on_serial_port_input = {
        let draft = draft.clone();
        Callback::from(move |event: InputEvent| {
            let value = event.target_unchecked_into::<HtmlInputElement>().value();
            update_draft(&draft, move |next| next.serial_port = value);
        })
    };
    let on_serial_baud_input = {
        let draft = draft.clone();
        Callback::from(move |event: InputEvent| {
            let value = event.target_unchecked_into::<HtmlInputElement>().value();
            update_draft(&draft, move |next| next.serial_baud = value);
        })
    };
    let on_jlink_enabled = {
        let draft = draft.clone();
        Callback::from(move |event: Event| {
            let checked = event.target_unchecked_into::<HtmlInputElement>().checked();
            update_draft(&draft, move |next| next.jlink_enabled = checked);
        })
    };
    let on_jlink_serial_input = {
        let draft = draft.clone();
        Callback::from(move |event: InputEvent| {
            let value = event.target_unchecked_into::<HtmlInputElement>().value();
            update_draft(&draft, move |next| next.jlink_serial = value);
        })
    };
    let on_jlink_device_input = {
        let draft = draft.clone();
        Callback::from(move |event: InputEvent| {
            let value = event.target_unchecked_into::<HtmlInputElement>().value();
            update_draft(&draft, move |next| next.jlink_device = value);
        })
    };
    let on_jlink_interface_input = {
        let draft = draft.clone();
        Callback::from(move |event: InputEvent| {
            let value = event.target_unchecked_into::<HtmlInputElement>().value();
            update_draft(&draft, move |next| next.jlink_interface = value);
        })
    };
    let on_jlink_speed_input = {
        let draft = draft.clone();
        Callback::from(move |event: InputEvent| {
            let value = event.target_unchecked_into::<HtmlInputElement>().value();
            update_draft(&draft, move |next| next.jlink_speed_khz = value);
        })
    };
    let on_jlink_autoconnect = {
        let draft = draft.clone();
        Callback::from(move |event: Event| {
            let checked = event.target_unchecked_into::<HtmlInputElement>().checked();
            update_draft(&draft, move |next| next.jlink_autoconnect = checked);
        })
    };

    let on_submit = {
        let draft = draft.clone();
        let form_error = form_error.clone();
        let notice = notice.clone();
        let devices = devices.clone();
        let page_error = page_error.clone();
        let editing_device_id = editing_device_id.clone();
        let submitting = submitting.clone();
        Callback::from(move |_| {
            let device = match (*draft).to_device() {
                Ok(device) => device,
                Err(message) => {
                    form_error.set(Some(message));
                    return;
                }
            };

            form_error.set(None);
            notice.set(None);
            submitting.set(true);

            let draft = draft.clone();
            let form_error = form_error.clone();
            let notice = notice.clone();
            let devices = devices.clone();
            let page_error = page_error.clone();
            let editing_device_id = editing_device_id.clone();
            let submitting = submitting.clone();
            let editing = (*editing_device_id).clone();
            spawn_local(async move {
                let result = if editing.is_some() {
                    api::update_device(&device.id, &device).await
                } else {
                    api::create_device(&device).await
                };

                match result {
                    Ok(saved) => {
                        let saved_id = saved.id.clone();
                        let saved_label = saved.label.clone();
                        editing_device_id.set(None);
                        draft.set(DeviceDraft::default());
                        form_error.set(None);
                        notice.set(Some(if editing.is_some() {
                            format!("Saved {saved_label} ({saved_id})")
                        } else {
                            format!("Registered {saved_label} ({saved_id})")
                        }));
                        refresh_devices(devices, page_error);
                    }
                    Err(message) => form_error.set(Some(message)),
                }
                submitting.set(false);
            });
        })
    };

    let on_reset = {
        let draft = draft.clone();
        let form_error = form_error.clone();
        let notice = notice.clone();
        let editing_device_id = editing_device_id.clone();
        Callback::from(move |_| {
            draft.set(DeviceDraft::default());
            form_error.set(None);
            notice.set(None);
            editing_device_id.set(None);
        })
    };

    let draft_value = (*draft).clone();
    let is_editing = (*editing_device_id).is_some();
    let is_submitting = *submitting;

    html! {
        <section class="grid gap-5 xl:grid-cols-[minmax(0,1.2fr)_minmax(0,0.8fr)]">
            <article class="rounded-[24px] border border-black/10 bg-white/80 p-6 shadow-shell backdrop-blur xl:row-span-2">
                <div class="flex flex-wrap items-start justify-between gap-4">
                    <div>
                        <span class="inline-flex items-center gap-2 uppercase tracking-[0.18em] text-[0.74rem] text-accent-strong">{"Registry"}</span>
                        <h2 class="mb-3 mt-3 text-2xl font-semibold tracking-tight text-ink">{"Managed devices"}</h2>
                        <p class="m-0 text-ink-muted">{"The control plane now persists device changes in its own registry file, so operators and AI callers can evolve targets without hand-editing startup config."}</p>
                    </div>
                    <button
                        class="rounded-2xl border border-black/10 bg-white/75 px-4 py-3 text-sm font-medium text-ink transition hover:bg-white disabled:cursor-default disabled:opacity-55"
                        onclick={on_reset.clone()}
                        disabled={is_submitting}
                    >
                        {if is_editing { "Exit edit" } else { "Clear form" }}
                    </button>
                </div>

                <div class="mt-6 grid gap-4">
                    {
                        if let Some(message) = (*page_error).clone() {
                            html! { <p class="m-0 text-sm text-red-700">{message}</p> }
                        } else if let Some(list) = (*devices).clone() {
                            if list.is_empty() {
                                html! { <p class="m-0 text-ink-muted">{"No devices registered yet. Use the form to create the first target."}</p> }
                            } else {
                                html! {
                                    for list.into_iter().map(|device| {
                                        let route = Route::DeviceDetail { device_id: device.id.clone() };
                                        let edit_draft = draft.clone();
                                        let edit_device_id = editing_device_id.clone();
                                        let edit_form_error = form_error.clone();
                                        let edit_notice = notice.clone();
                                        let delete_submitting = submitting.clone();
                                        let delete_devices = devices.clone();
                                        let delete_page_error = page_error.clone();
                                        let delete_draft = draft.clone();
                                        let delete_device_id_state = editing_device_id.clone();
                                        let delete_form_error = form_error.clone();
                                        let delete_notice = notice.clone();
                                        let delete_id = device.id.clone();
                                        let delete_label = device.label.clone();
                                        let is_currently_editing = (*editing_device_id).as_ref() == Some(&device.id);
                                        let capability_labels = [
                                            device.ssh.is_some().then_some("ssh"),
                                            device.serial.is_some().then_some("serial"),
                                            device.jlink.is_some().then_some("jlink"),
                                        ]
                                        .into_iter()
                                        .flatten()
                                        .collect::<Vec<_>>();

                                        let on_edit = {
                                            let device = device.clone();
                                            Callback::from(move |_| {
                                                edit_draft.set(DeviceDraft::from_device(&device));
                                                edit_device_id.set(Some(device.id.clone()));
                                                edit_form_error.set(None);
                                                edit_notice.set(Some(format!("Editing {} ({})", device.label, device.id)));
                                            })
                                        };

                                        let on_delete = Callback::from(move |_| {
                                            if !confirm_delete(&delete_id) {
                                                return;
                                            }
                                            delete_submitting.set(true);
                                            let devices = delete_devices.clone();
                                            let page_error = delete_page_error.clone();
                                            let draft = delete_draft.clone();
                                            let editing_device_id = delete_device_id_state.clone();
                                            let form_error = delete_form_error.clone();
                                            let notice = delete_notice.clone();
                                            let submitting = delete_submitting.clone();
                                            let delete_id = delete_id.clone();
                                            let delete_label = delete_label.clone();
                                            spawn_local(async move {
                                                match api::delete_device(&delete_id).await {
                                                    Ok(_) => {
                                                        if (*editing_device_id).as_ref() == Some(&delete_id) {
                                                            editing_device_id.set(None);
                                                            draft.set(DeviceDraft::default());
                                                        }
                                                        form_error.set(None);
                                                        notice.set(Some(format!("Removed {delete_label} ({delete_id})")));
                                                        refresh_devices(devices, page_error);
                                                    }
                                                    Err(message) => form_error.set(Some(message)),
                                                }
                                                submitting.set(false);
                                            });
                                        });

                                        html! {
                                            <div class="flex flex-col gap-4 rounded-[24px] border border-black/10 bg-slate-950/[0.03] p-4 sm:flex-row sm:items-center sm:justify-between">
                                                <div class="grid gap-2">
                                                    <div>
                                                        <h3 class="mb-1 mt-0 text-lg font-semibold text-ink">{device.label.clone()}</h3>
                                                        <p class="m-0 font-mono text-sm text-ink-muted">{device.id.clone()}</p>
                                                    </div>
                                                    <div class="flex flex-wrap items-center gap-2">
                                                        { for capability_labels.iter().map(|label| html! {
                                                            <span class="inline-flex rounded-full bg-accent/10 px-3 py-1.5 text-xs font-medium text-accent-strong">{*label}</span>
                                                        }) }
                                                        {
                                                            if capability_labels.is_empty() {
                                                                html! { <span class="inline-flex rounded-full bg-black/5 px-3 py-1.5 text-xs font-medium text-ink-muted">{"unconfigured"}</span> }
                                                            } else {
                                                                Html::default()
                                                            }
                                                        }
                                                        {
                                                            if is_currently_editing {
                                                                html! { <span class="inline-flex rounded-full bg-amber-100 px-3 py-1.5 text-xs font-medium text-amber-700">{"editing"}</span> }
                                                            } else {
                                                                Html::default()
                                                            }
                                                        }
                                                    </div>
                                                </div>
                                                <div class="flex flex-wrap items-center gap-3">
                                                    <button
                                                        class="rounded-2xl border border-black/10 bg-white/75 px-4 py-3 text-sm font-medium text-ink transition hover:bg-white disabled:cursor-default disabled:opacity-55"
                                                        onclick={on_edit}
                                                        disabled={is_submitting}
                                                    >{"Edit"}</button>
                                                    <button
                                                        class="rounded-2xl border border-red-200 bg-red-50 px-4 py-3 text-sm font-medium text-red-700 transition hover:bg-red-100 disabled:cursor-default disabled:opacity-55"
                                                        onclick={on_delete}
                                                        disabled={is_submitting}
                                                    >{"Remove"}</button>
                                                    <Link<Route>
                                                        classes="text-sm font-medium text-accent-strong transition hover:text-accent"
                                                        to={route}
                                                    >{"Open"}</Link<Route>>
                                                </div>
                                            </div>
                                        }
                                    })
                                }
                            }
                        } else {
                            html! { <p class="m-0 text-ink-muted">{"Loading devices..."}</p> }
                        }
                    }
                </div>
            </article>

            <article class="rounded-[24px] border border-black/10 bg-white/80 p-6 shadow-shell backdrop-blur">
                <span class="inline-flex items-center gap-2 uppercase tracking-[0.18em] text-[0.74rem] text-accent-strong">{"Planner"}</span>
                <h2 class="mb-3 mt-3 text-2xl font-semibold tracking-tight text-ink">
                    {if is_editing { "Edit target" } else { "Register target" }}
                </h2>
                <p class="m-0 text-ink-muted">{"Mix SSH, serial, and J-Link capabilities per board. The server validates these settings before saving them into the registry."}</p>
                {
                    if let Some(message) = (*notice).clone() {
                        html! { <p class="mb-0 mt-4 rounded-2xl bg-emerald-50 px-4 py-3 text-sm text-emerald-700">{message}</p> }
                    } else {
                        Html::default()
                    }
                }
                {
                    if let Some(message) = (*form_error).clone() {
                        html! { <p class="mb-0 mt-4 rounded-2xl bg-red-50 px-4 py-3 text-sm text-red-700">{message}</p> }
                    } else {
                        Html::default()
                    }
                }
            </article>

            <article class="rounded-[24px] border border-black/10 bg-white/80 p-6 shadow-shell backdrop-blur">
                <div class="grid gap-4">
                    <label class="grid gap-2 text-sm text-ink-muted">
                        <span>{"Device id"}</span>
                        <input
                            class="w-full rounded-2xl border border-black/10 bg-white/75 px-4 py-3 font-mono text-ink outline-none transition placeholder:text-ink-muted/70 focus:border-accent/40 focus:ring-2 focus:ring-accent/10 disabled:bg-slate-100"
                            value={draft_value.id.clone()}
                            oninput={on_id_input}
                            disabled={is_editing || is_submitting}
                            placeholder="board-gamma"
                        />
                    </label>
                    <label class="grid gap-2 text-sm text-ink-muted">
                        <span>{"Label"}</span>
                        <input
                            class="w-full rounded-2xl border border-black/10 bg-white/75 px-4 py-3 text-ink outline-none transition placeholder:text-ink-muted/70 focus:border-accent/40 focus:ring-2 focus:ring-accent/10"
                            value={draft_value.label.clone()}
                            oninput={on_label_input}
                            disabled={is_submitting}
                            placeholder="Bench UART"
                        />
                    </label>

                    <div class="grid gap-3 rounded-[24px] border border-black/10 bg-slate-950/[0.02] p-4">
                        <label class="flex items-center justify-between gap-3 text-sm font-medium text-ink">
                            <span>{"Enable SSH"}</span>
                            <input type="checkbox" checked={draft_value.ssh_enabled} onchange={on_ssh_enabled} disabled={is_submitting} />
                        </label>
                        {
                            if draft_value.ssh_enabled {
                                html! {
                                    <div class="grid gap-3">
                                        <input class="w-full rounded-2xl border border-black/10 bg-white/75 px-4 py-3 text-ink outline-none transition placeholder:text-ink-muted/70 focus:border-accent/40 focus:ring-2 focus:ring-accent/10" value={draft_value.ssh_host.clone()} oninput={on_ssh_host_input} disabled={is_submitting} placeholder="192.168.1.100" />
                                        <input class="w-full rounded-2xl border border-black/10 bg-white/75 px-4 py-3 text-ink outline-none transition placeholder:text-ink-muted/70 focus:border-accent/40 focus:ring-2 focus:ring-accent/10" value={draft_value.ssh_user.clone()} oninput={on_ssh_user_input} disabled={is_submitting} placeholder="root" />
                                        <input class="w-full rounded-2xl border border-black/10 bg-white/75 px-4 py-3 text-ink outline-none transition placeholder:text-ink-muted/70 focus:border-accent/40 focus:ring-2 focus:ring-accent/10" value={draft_value.ssh_key.clone()} oninput={on_ssh_key_input} disabled={is_submitting} placeholder="~/.ssh/id_rsa" />
                                    </div>
                                }
                            } else {
                                html! { <p class="m-0 text-sm text-ink-muted">{"Skip SSH if the target only needs serial and probe control."}</p> }
                            }
                        }
                    </div>

                    <div class="grid gap-3 rounded-[24px] border border-black/10 bg-slate-950/[0.02] p-4">
                        <label class="flex items-center justify-between gap-3 text-sm font-medium text-ink">
                            <span>{"Enable serial"}</span>
                            <input type="checkbox" checked={draft_value.serial_enabled} onchange={on_serial_enabled} disabled={is_submitting} />
                        </label>
                        {
                            if draft_value.serial_enabled {
                                html! {
                                    <div class="grid gap-3 sm:grid-cols-2">
                                        <input class="w-full rounded-2xl border border-black/10 bg-white/75 px-4 py-3 text-ink outline-none transition placeholder:text-ink-muted/70 focus:border-accent/40 focus:ring-2 focus:ring-accent/10" value={draft_value.serial_port.clone()} oninput={on_serial_port_input} disabled={is_submitting} placeholder="/dev/ttyUSB0" />
                                        <input class="w-full rounded-2xl border border-black/10 bg-white/75 px-4 py-3 text-ink outline-none transition placeholder:text-ink-muted/70 focus:border-accent/40 focus:ring-2 focus:ring-accent/10" value={draft_value.serial_baud.clone()} oninput={on_serial_baud_input} disabled={is_submitting} placeholder="115200" />
                                    </div>
                                }
                            } else {
                                html! { <p class="m-0 text-sm text-ink-muted">{"Enable serial when the board needs log streaming or command injection over UART."}</p> }
                            }
                        }
                    </div>

                    <div class="grid gap-3 rounded-[24px] border border-black/10 bg-slate-950/[0.02] p-4">
                        <label class="flex items-center justify-between gap-3 text-sm font-medium text-ink">
                            <span>{"Enable J-Link"}</span>
                            <input type="checkbox" checked={draft_value.jlink_enabled} onchange={on_jlink_enabled} disabled={is_submitting} />
                        </label>
                        {
                            if draft_value.jlink_enabled {
                                html! {
                                    <div class="grid gap-3">
                                        <div class="grid gap-3 sm:grid-cols-2">
                                            <input class="w-full rounded-2xl border border-black/10 bg-white/75 px-4 py-3 text-ink outline-none transition placeholder:text-ink-muted/70 focus:border-accent/40 focus:ring-2 focus:ring-accent/10" value={draft_value.jlink_serial.clone()} oninput={on_jlink_serial_input} disabled={is_submitting} placeholder="123456789" />
                                            <input class="w-full rounded-2xl border border-black/10 bg-white/75 px-4 py-3 text-ink outline-none transition placeholder:text-ink-muted/70 focus:border-accent/40 focus:ring-2 focus:ring-accent/10" value={draft_value.jlink_device.clone()} oninput={on_jlink_device_input} disabled={is_submitting} placeholder="STM32H743" />
                                        </div>
                                        <div class="grid gap-3 sm:grid-cols-2">
                                            <input class="w-full rounded-2xl border border-black/10 bg-white/75 px-4 py-3 text-ink outline-none transition placeholder:text-ink-muted/70 focus:border-accent/40 focus:ring-2 focus:ring-accent/10" value={draft_value.jlink_interface.clone()} oninput={on_jlink_interface_input} disabled={is_submitting} placeholder="SWD" />
                                            <input class="w-full rounded-2xl border border-black/10 bg-white/75 px-4 py-3 text-ink outline-none transition placeholder:text-ink-muted/70 focus:border-accent/40 focus:ring-2 focus:ring-accent/10" value={draft_value.jlink_speed_khz.clone()} oninput={on_jlink_speed_input} disabled={is_submitting} placeholder="4000" />
                                        </div>
                                        <label class="flex items-center gap-3 text-sm text-ink-muted">
                                            <input type="checkbox" checked={draft_value.jlink_autoconnect} onchange={on_jlink_autoconnect} disabled={is_submitting} />
                                            <span>{"Autoconnect after attach"}</span>
                                        </label>
                                    </div>
                                }
                            } else {
                                html! { <p class="m-0 text-sm text-ink-muted">{"Enable J-Link for probe, flash, verify, reset, and erase actions."}</p> }
                            }
                        }
                    </div>

                    <div class="flex flex-wrap justify-end gap-3">
                        <button
                            class="rounded-2xl border border-black/10 bg-white/75 px-4 py-3 text-sm font-medium text-ink transition hover:bg-white disabled:cursor-default disabled:opacity-55"
                            onclick={on_reset}
                            disabled={is_submitting}
                        >
                            {if is_editing { "Cancel edit" } else { "Reset" }}
                        </button>
                        <button
                            class="rounded-2xl bg-gradient-to-br from-accent to-accent-strong px-4 py-3 font-medium text-white transition hover:opacity-95 disabled:cursor-default disabled:opacity-55"
                            onclick={on_submit}
                            disabled={is_submitting}
                        >
                            {if is_submitting {
                                if is_editing { "Saving..." } else { "Registering..." }
                            } else if is_editing {
                                "Save device"
                            } else {
                                "Register device"
                            }}
                        </button>
                    </div>
                </div>
            </article>
        </section>
    }
}

fn update_draft(draft: &UseStateHandle<DeviceDraft>, update: impl FnOnce(&mut DeviceDraft)) {
    let mut next = (**draft).clone();
    update(&mut next);
    draft.set(next);
}

fn refresh_devices(devices: UseStateHandle<Option<Vec<Device>>>, page_error: UseStateHandle<Option<String>>) {
    spawn_local(async move {
        match api::list_devices().await {
            Ok(list) => {
                devices.set(Some(list));
                page_error.set(None);
            }
            Err(message) => page_error.set(Some(message)),
        }
    });
}

fn confirm_delete(device_id: &str) -> bool {
    web_sys::window()
        .and_then(|window| {
            window
                .confirm_with_message(&format!("Remove device {device_id}?"))
                .ok()
        })
        .unwrap_or(false)
}
