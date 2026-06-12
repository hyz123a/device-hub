use wasm_bindgen_futures::spawn_local;
use web_sys::{HtmlInputElement, HtmlSelectElement};
use yew::prelude::*;
use yew_router::prelude::Link;

use crate::{api, app::Route};
use device_hub_shared::{
    AdbConfig, BoardProfile, Device, DiscoveredJlinkProbe, DiscoveredSerialPort, JlinkConfig,
    SerialConfig, SshConfig, default_adb_logcat_args, default_ssh_log_path,
};

#[derive(Clone, PartialEq)]
struct DeviceDraft {
    id: String,
    label: String,
    ssh_enabled: bool,
    ssh_host: String,
    ssh_user: String,
    ssh_key: String,
    ssh_default_log_path: String,
    serial_enabled: bool,
    serial_port: String,
    serial_baud: String,
    jlink_enabled: bool,
    selected_jlink_board_profile_id: Option<String>,
    jlink_serial: String,
    jlink_device: String,
    jlink_interface: String,
    jlink_speed_khz: String,
    jlink_autoconnect: bool,
    adb_enabled: bool,
    adb_serial: String,
    adb_transport: String,
    adb_default_logcat_args: String,
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
            ssh_default_log_path: default_ssh_log_path(),
            serial_enabled: false,
            serial_port: String::new(),
            serial_baud: "115200".into(),
            jlink_enabled: false,
            selected_jlink_board_profile_id: None,
            jlink_serial: String::new(),
            jlink_device: String::new(),
            jlink_interface: "SWD".into(),
            jlink_speed_khz: "4000".into(),
            jlink_autoconnect: true,
            adb_enabled: false,
            adb_serial: String::new(),
            adb_transport: "serial".into(),
            adb_default_logcat_args: default_adb_logcat_args().join(" "),
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
            draft.ssh_default_log_path = ssh.default_log_path.clone();
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
        if let Some(adb) = &device.adb {
            draft.adb_enabled = true;
            draft.adb_serial = adb.serial.clone();
            draft.adb_transport = adb.transport.clone().unwrap_or_else(|| "serial".into());
            draft.adb_default_logcat_args = adb.default_logcat_args.join(" ");
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
        if !self.ssh_enabled && !self.serial_enabled && !self.jlink_enabled && !self.adb_enabled {
            return Err("Enable at least one connection before saving".into());
        }

        let ssh = if self.ssh_enabled {
            let host = self.ssh_host.trim().to_owned();
            let user = self.ssh_user.trim().to_owned();
            let key = self.ssh_key.trim().to_owned();
            let default_log_path = {
                let value = self.ssh_default_log_path.trim();
                if value.is_empty() {
                    default_ssh_log_path()
                } else {
                    value.to_owned()
                }
            };
            if host.is_empty() || user.is_empty() || key.is_empty() {
                return Err("SSH config requires host, user, and key".into());
            }
            Some(SshConfig {
                host,
                user,
                key,
                default_log_path,
            })
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

        let adb = if self.adb_enabled {
            let serial = self.adb_serial.trim().to_owned();
            if serial.is_empty() {
                return Err("ADB config requires a device serial".into());
            }
            let transport = self.adb_transport.trim();
            if !matches!(transport, "serial" | "usb" | "emulator") {
                return Err("ADB transport must be serial, usb, or emulator".into());
            }
            let default_logcat_args = split_args_or_default(&self.adb_default_logcat_args);
            Some(AdbConfig {
                serial,
                transport: (transport != "serial").then(|| transport.to_owned()),
                default_logcat_args,
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
            adb,
        })
    }

    fn select_serial_port(&mut self, path: String) {
        self.serial_port = path;
    }

    fn select_jlink_probe(&mut self, serial: String) {
        self.jlink_serial = serial;
    }

    fn apply_jlink_board_profile(&mut self, profile: &BoardProfile) {
        self.selected_jlink_board_profile_id = Some(profile.id.clone());
        self.jlink_device = profile.jlink.device.clone();
        self.jlink_interface = profile.jlink.interface.clone();
        self.jlink_speed_khz = profile.jlink.speed_khz.to_string();
    }

    fn clear_jlink_board_profile_selection(&mut self) {
        self.selected_jlink_board_profile_id = None;
        self.jlink_device.clear();
        self.jlink_interface = "SWD".into();
        self.jlink_speed_khz = "4000".into();
    }

    fn mark_jlink_fields_as_manual(&mut self) {
        self.selected_jlink_board_profile_id = None;
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
    let board_profiles = use_state(|| Option::<Vec<BoardProfile>>::None);
    let board_profiles_error = use_state(|| Option::<String>::None);
    let board_profiles_loading = use_state(|| false);
    let serial_ports = use_state(|| Option::<Vec<DiscoveredSerialPort>>::None);
    let serial_scan_error = use_state(|| Option::<String>::None);
    let serial_scan_loading = use_state(|| false);
    let jlink_probes = use_state(|| Option::<Vec<DiscoveredJlinkProbe>>::None);
    let jlink_scan_error = use_state(|| Option::<String>::None);
    let jlink_scan_loading = use_state(|| false);

    {
        let devices = devices.clone();
        let page_error = page_error.clone();
        let board_profiles = board_profiles.clone();
        let board_profiles_error = board_profiles_error.clone();
        let board_profiles_loading = board_profiles_loading.clone();
        let serial_ports = serial_ports.clone();
        let serial_scan_error = serial_scan_error.clone();
        let serial_scan_loading = serial_scan_loading.clone();
        let jlink_probes = jlink_probes.clone();
        let jlink_scan_error = jlink_scan_error.clone();
        let jlink_scan_loading = jlink_scan_loading.clone();
        use_effect_with((), move |_| {
            refresh_devices(devices.clone(), page_error.clone());
            refresh_board_profiles(
                board_profiles.clone(),
                board_profiles_error.clone(),
                board_profiles_loading.clone(),
            );
            refresh_serial_ports(
                serial_ports.clone(),
                serial_scan_error.clone(),
                serial_scan_loading.clone(),
            );
            refresh_jlink_probes(
                jlink_probes.clone(),
                jlink_scan_error.clone(),
                jlink_scan_loading.clone(),
            );
            || {}
        });
    }

    {
        let draft = draft.clone();
        let jlink_probes = (*jlink_probes).clone();
        use_effect_with(
            (jlink_probes, (*draft).jlink_serial.clone()),
            move |(probes, serial)| {
                if let Some(autofill_serial) = autofill_jlink_serial(probes.as_deref(), serial) {
                    update_draft(&draft, move |next| next.select_jlink_probe(autofill_serial));
                }
                || {}
            },
        );
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
    let on_ssh_default_log_path_input = {
        let draft = draft.clone();
        Callback::from(move |event: InputEvent| {
            let value = event.target_unchecked_into::<HtmlInputElement>().value();
            update_draft(&draft, move |next| next.ssh_default_log_path = value);
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
    let on_serial_scan_select = {
        let draft = draft.clone();
        Callback::from(move |event: Event| {
            let value = event.target_unchecked_into::<HtmlSelectElement>().value();
            if value.is_empty() {
                return;
            }
            update_draft(&draft, move |next| next.select_serial_port(value));
        })
    };
    let on_refresh_serial_scan = {
        let serial_ports = serial_ports.clone();
        let serial_scan_error = serial_scan_error.clone();
        let serial_scan_loading = serial_scan_loading.clone();
        Callback::from(move |_| {
            refresh_serial_ports(
                serial_ports.clone(),
                serial_scan_error.clone(),
                serial_scan_loading.clone(),
            );
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
            update_draft(&draft, move |next| {
                next.mark_jlink_fields_as_manual();
                next.jlink_device = value;
            });
        })
    };
    let on_jlink_interface_input = {
        let draft = draft.clone();
        Callback::from(move |event: InputEvent| {
            let value = event.target_unchecked_into::<HtmlInputElement>().value();
            update_draft(&draft, move |next| {
                next.mark_jlink_fields_as_manual();
                next.jlink_interface = value;
            });
        })
    };
    let on_jlink_speed_input = {
        let draft = draft.clone();
        Callback::from(move |event: InputEvent| {
            let value = event.target_unchecked_into::<HtmlInputElement>().value();
            update_draft(&draft, move |next| {
                next.mark_jlink_fields_as_manual();
                next.jlink_speed_khz = value;
            });
        })
    };
    let on_jlink_autoconnect = {
        let draft = draft.clone();
        Callback::from(move |event: Event| {
            let checked = event.target_unchecked_into::<HtmlInputElement>().checked();
            update_draft(&draft, move |next| next.jlink_autoconnect = checked);
        })
    };
    let on_jlink_scan_select = {
        let draft = draft.clone();
        Callback::from(move |event: Event| {
            let value = event.target_unchecked_into::<HtmlSelectElement>().value();
            if value.is_empty() {
                return;
            }
            update_draft(&draft, move |next| next.select_jlink_probe(value));
        })
    };
    let on_jlink_board_profile_select = {
        let draft = draft.clone();
        let board_profiles = board_profiles.clone();
        Callback::from(move |event: Event| {
            let value = event.target_unchecked_into::<HtmlSelectElement>().value();
            if value.is_empty() {
                update_draft(&draft, |next| next.clear_jlink_board_profile_selection());
                return;
            }
            let Some(profile) = lookup_jlink_board_profile((*board_profiles).as_deref(), &value)
            else {
                return;
            };
            update_draft(&draft, move |next| next.apply_jlink_board_profile(profile));
        })
    };
    let on_refresh_jlink_scan = {
        let jlink_probes = jlink_probes.clone();
        let jlink_scan_error = jlink_scan_error.clone();
        let jlink_scan_loading = jlink_scan_loading.clone();
        Callback::from(move |_| {
            refresh_jlink_probes(
                jlink_probes.clone(),
                jlink_scan_error.clone(),
                jlink_scan_loading.clone(),
            );
        })
    };
    let on_adb_enabled = {
        let draft = draft.clone();
        Callback::from(move |event: Event| {
            let checked = event.target_unchecked_into::<HtmlInputElement>().checked();
            update_draft(&draft, move |next| next.adb_enabled = checked);
        })
    };
    let on_adb_serial_input = {
        let draft = draft.clone();
        Callback::from(move |event: InputEvent| {
            let value = event.target_unchecked_into::<HtmlInputElement>().value();
            update_draft(&draft, move |next| next.adb_serial = value);
        })
    };
    let on_adb_transport_select = {
        let draft = draft.clone();
        Callback::from(move |event: Event| {
            let value = event.target_unchecked_into::<HtmlSelectElement>().value();
            update_draft(&draft, move |next| next.adb_transport = value);
        })
    };
    let on_adb_logcat_args_input = {
        let draft = draft.clone();
        Callback::from(move |event: InputEvent| {
            let value = event.target_unchecked_into::<HtmlInputElement>().value();
            update_draft(&draft, move |next| next.adb_default_logcat_args = value);
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
    let selected_serial_scan =
        selected_serial_scan_value(&draft_value.serial_port, (*serial_ports).as_deref());
    let selected_jlink_scan =
        selected_jlink_scan_value(&draft_value.jlink_serial, (*jlink_probes).as_deref());
    let selected_jlink_board_profile = draft_value
        .selected_jlink_board_profile_id
        .clone()
        .unwrap_or_default();

    html! {
        <section class="grid gap-5 xl:grid-cols-[minmax(0,1.2fr)_minmax(0,0.8fr)]">
            <article class="dh-panel rounded-[24px] p-6 xl:row-span-2">
                <div class="flex flex-wrap items-start justify-between gap-4">
                    <div>
                        <span class="inline-flex items-center gap-2 uppercase tracking-[0.18em] text-[0.74rem] text-accent-strong">{"Registry"}</span>
                        <h2 class="mb-3 mt-3 text-2xl font-semibold tracking-tight text-ink">{"Managed devices"}</h2>
                        <p class="m-0 text-ink-muted">{"The control plane now persists device changes in its own registry file, so operators and AI callers can evolve targets without hand-editing startup config."}</p>
                    </div>
                    <button
                        class="rounded-2xl dh-button px-4 py-3 text-sm"
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
                                        let logs_route = Route::DeviceLogs {
                                            device_id: device.id.clone(),
                                        };
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
                                            device.adb.is_some().then_some("adb"),
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
                                            <div class="flex flex-col gap-4 dh-surface-muted rounded-[24px] p-4 sm:flex-row sm:items-center sm:justify-between">
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
                                                                html! { <span class="inline-flex rounded-full dh-pill px-3 py-1.5 text-xs font-medium text-ink-muted">{"unconfigured"}</span> }
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
                                                        class="rounded-2xl dh-button px-4 py-3 text-sm"
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
                                                        to={logs_route}
                                                    >{"Logs"}</Link<Route>>
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

            <article class="dh-panel rounded-[24px] p-6">
                <span class="inline-flex items-center gap-2 uppercase tracking-[0.18em] text-[0.74rem] text-accent-strong">{"Planner"}</span>
                <h2 class="mb-3 mt-3 text-2xl font-semibold tracking-tight text-ink">
                    {if is_editing { "Edit target" } else { "Register target" }}
                </h2>
                <p class="m-0 text-ink-muted">{"Mix SSH, serial, J-Link, and ADB capabilities per target. The server validates these settings before saving them into the registry."}</p>
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

            <article class="dh-panel rounded-[24px] p-6">
                <div class="grid gap-4">
                    <label class="grid gap-2 text-sm text-ink-muted">
                        <span>{"Device id"}</span>
                        <input
                            class="w-full rounded-2xl dh-input font-mono"
                            value={draft_value.id.clone()}
                            oninput={on_id_input}
                            disabled={is_editing || is_submitting}
                            placeholder="board-gamma"
                        />
                    </label>
                    <label class="grid gap-2 text-sm text-ink-muted">
                        <span>{"Label"}</span>
                        <input
                            class="w-full rounded-2xl dh-input"
                            value={draft_value.label.clone()}
                            oninput={on_label_input}
                            disabled={is_submitting}
                            placeholder="Bench UART"
                        />
                    </label>

                    <div class="grid gap-3 dh-surface-muted rounded-[24px] p-4">
                        <label class="flex items-center justify-between gap-3 text-sm font-medium text-ink">
                            <span>{"Enable SSH"}</span>
                            <input type="checkbox" checked={draft_value.ssh_enabled} onchange={on_ssh_enabled} disabled={is_submitting} />
                        </label>
                        {
                            if draft_value.ssh_enabled {
                                html! {
                                     <div class="grid gap-3">
                                         <input class="w-full rounded-2xl dh-input" value={draft_value.ssh_host.clone()} oninput={on_ssh_host_input} disabled={is_submitting} placeholder="192.168.1.100" />
                                         <input class="w-full rounded-2xl dh-input" value={draft_value.ssh_user.clone()} oninput={on_ssh_user_input} disabled={is_submitting} placeholder="root" />
                                         <input class="w-full rounded-2xl dh-input" value={draft_value.ssh_key.clone()} oninput={on_ssh_key_input} disabled={is_submitting} placeholder="~/.ssh/id_rsa" />
                                         <input class="w-full rounded-2xl dh-input" value={draft_value.ssh_default_log_path.clone()} oninput={on_ssh_default_log_path_input} disabled={is_submitting} placeholder="/tmp/sdcard/debug_log" />
                                     </div>
                                 }
                             } else {
                                 html! { <p class="m-0 text-sm text-ink-muted">{"Skip SSH if the target only needs serial and probe control."}</p> }
                            }
                        }
                    </div>

                    <div class="grid gap-3 dh-surface-muted rounded-[24px] p-4">
                        <label class="flex items-center justify-between gap-3 text-sm font-medium text-ink">
                            <span>{"Enable serial"}</span>
                            <input type="checkbox" checked={draft_value.serial_enabled} onchange={on_serial_enabled} disabled={is_submitting} />
                        </label>
                        {
                            if draft_value.serial_enabled {
                                html! {
                                    <div class="grid gap-3">
                                        <div class="flex flex-wrap items-center justify-between gap-3 rounded-2xl dh-surface px-4 py-3 text-sm text-ink-muted">
                                            <span>{"Scan this host for serial character devices, then keep editing the path manually if needed."}</span>
                                            <button
                                                class="rounded-2xl dh-button px-4 py-2 text-sm"
                                                onclick={on_refresh_serial_scan.clone()}
                                                disabled={is_submitting || *serial_scan_loading}
                                            >
                                                {if *serial_scan_loading { "Scanning serial..." } else { "Scan serial ports" }}
                                            </button>
                                        </div>
                                        {
                                            if let Some(message) = (*serial_scan_error).clone() {
                                                html! { <p class="m-0 rounded-2xl bg-red-50 px-4 py-3 text-sm text-red-700">{message}</p> }
                                            } else if let Some(ports) = (*serial_ports).clone() {
                                                if ports.is_empty() {
                                                    html! { <p class="m-0 text-sm text-ink-muted">{"No serial devices were detected on this host. You can still type the path manually."}</p> }
                                                } else {
                                                    html! {
                                                        <label class="grid gap-2 text-sm text-ink-muted">
                                                            <span>{"Scanned serial paths"}</span>
                                                            <select
                                                                class="w-full rounded-2xl dh-input"
                                                                onchange={on_serial_scan_select}
                                                                disabled={is_submitting}
                                                            >
                                                                <option value="" selected={selected_serial_scan.is_empty()}>{"Keep manual entry"}</option>
                                                                { for ports.iter().map(|port| {
                                                                    let is_selected = selected_serial_scan == port.path;
                                                                    html! {
                                                                        <option value={port.path.clone()} selected={is_selected}>{format_serial_port_option(port)}</option>
                                                                    }
                                                                }) }
                                                            </select>
                                                        </label>
                                                    }
                                                }
                                            } else {
                                                Html::default()
                                            }
                                        }
                                        <div class="grid gap-3 sm:grid-cols-2">
                                            <input class="w-full rounded-2xl dh-input" value={draft_value.serial_port.clone()} oninput={on_serial_port_input} disabled={is_submitting} placeholder="/dev/ttyUSB0" />
                                            <input class="w-full rounded-2xl dh-input" value={draft_value.serial_baud.clone()} oninput={on_serial_baud_input} disabled={is_submitting} placeholder="115200" />
                                        </div>
                                    </div>
                                }
                            } else {
                                html! { <p class="m-0 text-sm text-ink-muted">{"Enable serial when the board needs log streaming or command injection over UART."}</p> }
                            }
                        }
                    </div>

                    <div class="grid gap-3 dh-surface-muted rounded-[24px] p-4">
                        <label class="flex items-center justify-between gap-3 text-sm font-medium text-ink">
                            <span>{"Enable J-Link"}</span>
                            <input type="checkbox" checked={draft_value.jlink_enabled} onchange={on_jlink_enabled} disabled={is_submitting} />
                        </label>
                        {
                            if draft_value.jlink_enabled {
                                html! {
                                    <div class="grid gap-3">
                                        <div class="flex flex-wrap items-center justify-between gap-3 rounded-2xl dh-surface px-4 py-3 text-sm text-ink-muted">
                                            <span>{"Scan the host for attached probes, then keep the serial field editable for manual overrides."}</span>
                                            <button
                                                class="rounded-2xl dh-button px-4 py-2 text-sm"
                                                onclick={on_refresh_jlink_scan.clone()}
                                                disabled={is_submitting || *jlink_scan_loading}
                                            >
                                                {if *jlink_scan_loading { "Scanning probes..." } else { "Scan J-Link probes" }}
                                            </button>
                                        </div>
                                        {
                                            if let Some(message) = (*jlink_scan_error).clone() {
                                                html! { <p class="m-0 rounded-2xl bg-red-50 px-4 py-3 text-sm text-red-700">{message}</p> }
                                            } else if let Some(probes) = (*jlink_probes).clone() {
                                                if probes.is_empty() {
                                                    html! { <p class="m-0 text-sm text-ink-muted">{"No J-Link probes were detected on this host. You can still type the probe serial manually."}</p> }
                                                } else {
                                                    html! {
                                                        <label class="grid gap-2 text-sm text-ink-muted">
                                                            <span>{"Detected probes"}</span>
                                                            <select
                                                                class="w-full rounded-2xl dh-input"
                                                                onchange={on_jlink_scan_select}
                                                                disabled={is_submitting}
                                                            >
                                                                <option value="" selected={selected_jlink_scan.is_empty()}>{"Keep manual entry"}</option>
                                                                { for probes.iter().map(|probe| {
                                                                    let is_selected = selected_jlink_scan == probe.serial;
                                                                    html! {
                                                                        <option value={probe.serial.clone()} selected={is_selected}>{format_jlink_probe_option(probe)}</option>
                                                                    }
                                                                }) }
                                                            </select>
                                                        </label>
                                                    }
                                                }
                                            } else {
                                                Html::default()
                                            }
                                        }
                                        {
                                            if let Some(message) = (*board_profiles_error).clone() {
                                                html! { <p class="m-0 rounded-2xl bg-red-50 px-4 py-3 text-sm text-red-700">{message}</p> }
                                            } else if let Some(profiles) = (*board_profiles).clone() {
                                                html! {
                                                    <label class="grid gap-2 text-sm text-ink-muted">
                                                        <span>{"Board profile"}</span>
                                                        <select
                                                            class="w-full rounded-2xl dh-input"
                                                            onchange={on_jlink_board_profile_select}
                                                            disabled={is_submitting}
                                                        >
                                                            <option value="" selected={selected_jlink_board_profile.is_empty()}>{"Custom / manual"}</option>
                                                            { for profiles.iter().map(|profile| {
                                                                let is_selected = selected_jlink_board_profile == profile.id;
                                                                html! {
                                                                    <option value={profile.id.clone()} selected={is_selected}>{format_jlink_board_profile_option(profile)}</option>
                                                                }
                                                            }) }
                                                        </select>
                                                    </label>
                                                }
                                            } else if *board_profiles_loading {
                                                html! { <p class="m-0 text-sm text-ink-muted">{"Loading board profiles..."}</p> }
                                            } else {
                                                Html::default()
                                            }
                                        }
                                        <div class="grid gap-3 sm:grid-cols-2">
                                            <input class="w-full rounded-2xl dh-input" value={draft_value.jlink_serial.clone()} oninput={on_jlink_serial_input} disabled={is_submitting} placeholder="123456789" />
                                            <input class="w-full rounded-2xl dh-input" value={draft_value.jlink_device.clone()} oninput={on_jlink_device_input} disabled={is_submitting} placeholder="STM32H743" />
                                        </div>
                                        <div class="grid gap-3 sm:grid-cols-2">
                                            <input class="w-full rounded-2xl dh-input" value={draft_value.jlink_interface.clone()} oninput={on_jlink_interface_input} disabled={is_submitting} placeholder="SWD" />
                                            <input class="w-full rounded-2xl dh-input" value={draft_value.jlink_speed_khz.clone()} oninput={on_jlink_speed_input} disabled={is_submitting} placeholder="4000" />
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

                    <div class="grid gap-3 dh-surface-muted rounded-[24px] p-4">
                        <label class="flex items-center justify-between gap-3 text-sm font-medium text-ink">
                            <span>{"Enable ADB"}</span>
                            <input type="checkbox" checked={draft_value.adb_enabled} onchange={on_adb_enabled} disabled={is_submitting} />
                        </label>
                        {
                            if draft_value.adb_enabled {
                                html! {
                                    <div class="grid gap-3">
                                        <div class="grid gap-3 sm:grid-cols-2">
                                            <input class="w-full rounded-2xl dh-input font-mono" value={draft_value.adb_serial.clone()} oninput={on_adb_serial_input} disabled={is_submitting} placeholder="08211FDD4002KE" />
                                            <select
                                                class="w-full rounded-2xl dh-input"
                                                onchange={on_adb_transport_select}
                                                disabled={is_submitting}
                                            >
                                                <option value="serial" selected={draft_value.adb_transport == "serial"}>{"Device serial (-s)"}</option>
                                                <option value="usb" selected={draft_value.adb_transport == "usb"}>{"USB device (-d)"}</option>
                                                <option value="emulator" selected={draft_value.adb_transport == "emulator"}>{"Emulator (-e)"}</option>
                                            </select>
                                        </div>
                                        <input class="w-full rounded-2xl dh-input font-mono" value={draft_value.adb_default_logcat_args.clone()} oninput={on_adb_logcat_args_input} disabled={is_submitting} placeholder="-v time" />
                                    </div>
                                }
                            } else {
                                html! { <p class="m-0 text-sm text-ink-muted">{"Enable ADB when the target needs Android logcat streaming."}</p> }
                            }
                        }
                    </div>

                    <div class="flex flex-wrap justify-end gap-3">
                        <button
                            class="rounded-2xl dh-button px-4 py-3 text-sm"
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

fn refresh_devices(
    devices: UseStateHandle<Option<Vec<Device>>>,
    page_error: UseStateHandle<Option<String>>,
) {
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

fn refresh_serial_ports(
    serial_ports: UseStateHandle<Option<Vec<DiscoveredSerialPort>>>,
    serial_scan_error: UseStateHandle<Option<String>>,
    serial_scan_loading: UseStateHandle<bool>,
) {
    serial_scan_loading.set(true);
    spawn_local(async move {
        match api::list_serial_ports().await {
            Ok(ports) => {
                serial_ports.set(Some(ports));
                serial_scan_error.set(None);
            }
            Err(message) => serial_scan_error.set(Some(message)),
        }
        serial_scan_loading.set(false);
    });
}

fn refresh_board_profiles(
    board_profiles: UseStateHandle<Option<Vec<BoardProfile>>>,
    board_profiles_error: UseStateHandle<Option<String>>,
    board_profiles_loading: UseStateHandle<bool>,
) {
    board_profiles_loading.set(true);
    spawn_local(async move {
        match api::list_board_profiles().await {
            Ok(profiles) => {
                board_profiles.set(Some(profiles));
                board_profiles_error.set(None);
            }
            Err(message) => board_profiles_error.set(Some(message)),
        }
        board_profiles_loading.set(false);
    });
}

fn refresh_jlink_probes(
    jlink_probes: UseStateHandle<Option<Vec<DiscoveredJlinkProbe>>>,
    jlink_scan_error: UseStateHandle<Option<String>>,
    jlink_scan_loading: UseStateHandle<bool>,
) {
    jlink_scan_loading.set(true);
    spawn_local(async move {
        match api::list_jlink_probes().await {
            Ok(probes) => {
                jlink_probes.set(Some(probes));
                jlink_scan_error.set(None);
            }
            Err(message) => jlink_scan_error.set(Some(message)),
        }
        jlink_scan_loading.set(false);
    });
}

fn selected_serial_scan_value(current: &str, ports: Option<&[DiscoveredSerialPort]>) -> String {
    ports
        .is_some_and(|ports| ports.iter().any(|port| port.path == current))
        .then(|| current.to_owned())
        .unwrap_or_default()
}

fn selected_jlink_scan_value(current: &str, probes: Option<&[DiscoveredJlinkProbe]>) -> String {
    probes
        .is_some_and(|probes| probes.iter().any(|probe| probe.serial == current))
        .then(|| current.to_owned())
        .unwrap_or_default()
}

fn lookup_jlink_board_profile<'a>(
    profiles: Option<&'a [BoardProfile]>,
    profile_id: &str,
) -> Option<&'a BoardProfile> {
    profiles?.iter().find(|profile| profile.id == profile_id)
}

fn format_serial_port_option(port: &DiscoveredSerialPort) -> String {
    port.label
        .as_ref()
        .map(|label| format!("{} ({label})", port.path))
        .unwrap_or_else(|| port.path.clone())
}

fn format_jlink_board_profile_option(profile: &BoardProfile) -> String {
    format!(
        "{} ({} / {} / {} kHz)",
        profile.label, profile.jlink.device, profile.jlink.interface, profile.jlink.speed_khz
    )
}

fn format_jlink_probe_option(probe: &DiscoveredJlinkProbe) -> String {
    probe
        .product
        .as_ref()
        .map(|product| format!("{} ({product})", probe.serial))
        .unwrap_or_else(|| probe.serial.clone())
}

fn split_args_or_default(args: &str) -> Vec<String> {
    let parsed = args
        .split_whitespace()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if parsed.is_empty() {
        default_adb_logcat_args()
    } else {
        parsed
    }
}

fn autofill_jlink_serial(
    probes: Option<&[DiscoveredJlinkProbe]>,
    current_serial: &str,
) -> Option<String> {
    if !current_serial.trim().is_empty() {
        return None;
    }

    let mut valid_probes = probes?
        .iter()
        .filter(|probe| is_valid_jlink_serial(&probe.serial));
    let probe = valid_probes.next()?;
    valid_probes.next().is_none().then(|| probe.serial.clone())
}

fn is_valid_jlink_serial(serial: &str) -> bool {
    let serial = serial.trim();
    !serial.is_empty() && serial != "-1"
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

#[cfg(test)]
mod tests {
    use super::*;

    fn ssh_ready_draft() -> DeviceDraft {
        DeviceDraft {
            id: "board-alpha".into(),
            label: "Linux Board".into(),
            ssh_enabled: true,
            ssh_host: "192.168.1.100".into(),
            ssh_user: "root".into(),
            ssh_key: "~/.ssh/id_rsa".into(),
            ..DeviceDraft::default()
        }
    }

    fn serial_ready_draft() -> DeviceDraft {
        DeviceDraft {
            id: "board-gamma".into(),
            label: "Bench UART".into(),
            serial_enabled: true,
            serial_baud: "115200".into(),
            ..DeviceDraft::default()
        }
    }

    fn jlink_ready_draft() -> DeviceDraft {
        DeviceDraft {
            id: "board-zeta".into(),
            label: "Probe Target".into(),
            jlink_enabled: true,
            jlink_device: "STM32H743".into(),
            jlink_interface: "SWD".into(),
            jlink_speed_khz: "4000".into(),
            ..DeviceDraft::default()
        }
    }

    fn adb_ready_draft() -> DeviceDraft {
        DeviceDraft {
            id: "pixel5".into(),
            label: "vicohome".into(),
            adb_enabled: true,
            adb_serial: "08211FDD4002KE".into(),
            adb_default_logcat_args: "-v time".into(),
            ..DeviceDraft::default()
        }
    }

    fn sample_board_profile() -> BoardProfile {
        BoardProfile {
            id: "brd4186c".into(),
            label: "BRD4186C".into(),
            jlink: device_hub_shared::JlinkBoardProfile {
                device: "EFR32MG24BXXXF1536".into(),
                interface: "SWD".into(),
                speed_khz: 2700,
            },
        }
    }

    #[test]
    fn serial_scan_selection_can_be_overridden_manually() {
        let mut draft = serial_ready_draft();

        draft.select_serial_port("/dev/ttyUSB3".into());
        let scanned = draft.to_device().expect("serial device from scan");
        assert_eq!(
            scanned.serial.expect("serial config from scan").port,
            "/dev/ttyUSB3"
        );

        draft.serial_port = "/tmp/manual-uart".into();
        let manual = draft
            .to_device()
            .expect("serial device from manual override");
        assert_eq!(
            manual
                .serial
                .expect("serial config from manual override")
                .port,
            "/tmp/manual-uart"
        );
    }

    #[test]
    fn jlink_scan_selection_can_be_overridden_manually() {
        let mut draft = jlink_ready_draft();

        draft.select_jlink_probe("123456789".into());
        let scanned = draft.to_device().expect("jlink device from scan");
        assert_eq!(
            scanned.jlink.expect("jlink config from scan").serial,
            "123456789"
        );

        draft.jlink_serial = "000111222".into();
        let manual = draft
            .to_device()
            .expect("jlink device from manual override");
        assert_eq!(
            manual
                .jlink
                .expect("jlink config from manual override")
                .serial,
            "000111222"
        );
    }

    #[test]
    fn autofills_single_valid_jlink_probe_when_serial_is_empty() {
        let serial = autofill_jlink_serial(
            Some(&[
                DiscoveredJlinkProbe {
                    serial: "-1".into(),
                    product: Some("J-Link (unknown)".into()),
                },
                DiscoveredJlinkProbe {
                    serial: "150712169".into(),
                    product: Some("J-Link".into()),
                },
            ]),
            "",
        );

        assert_eq!(serial.as_deref(), Some("150712169"));
    }

    #[test]
    fn does_not_autofill_when_user_already_entered_serial() {
        let serial = autofill_jlink_serial(
            Some(&[DiscoveredJlinkProbe {
                serial: "150712169".into(),
                product: Some("J-Link".into()),
            }]),
            "manual-sn",
        );

        assert_eq!(serial, None);
    }

    #[test]
    fn board_profile_applies_known_jlink_defaults() {
        let mut draft = jlink_ready_draft();
        let profile = sample_board_profile();

        draft.apply_jlink_board_profile(&profile);

        assert_eq!(draft.jlink_device, "EFR32MG24BXXXF1536");
        assert_eq!(draft.jlink_interface, "SWD");
        assert_eq!(draft.jlink_speed_khz, "2700");
    }

    #[test]
    fn selected_board_profile_resets_to_custom_after_manual_override() {
        let mut draft = jlink_ready_draft();
        let profile = sample_board_profile();

        draft.apply_jlink_board_profile(&profile);
        assert_eq!(
            draft.selected_jlink_board_profile_id.as_deref(),
            Some("brd4186c")
        );

        draft.mark_jlink_fields_as_manual();
        draft.jlink_speed_khz = "4000".into();
        assert_eq!(draft.selected_jlink_board_profile_id, None);
    }

    #[test]
    fn explicit_custom_selection_clears_profile_defaults() {
        let mut draft = jlink_ready_draft();
        let profile = sample_board_profile();

        draft.apply_jlink_board_profile(&profile);
        draft.clear_jlink_board_profile_selection();

        assert_eq!(draft.selected_jlink_board_profile_id, None);
        assert_eq!(draft.jlink_device, "");
        assert_eq!(draft.jlink_interface, "SWD");
        assert_eq!(draft.jlink_speed_khz, "4000");
    }

    #[test]
    fn ssh_devices_default_log_path_when_field_is_blank() {
        let mut draft = ssh_ready_draft();
        draft.ssh_default_log_path.clear();

        let device = draft.to_device().expect("ssh device");
        assert_eq!(
            device.ssh.expect("ssh config").default_log_path,
            "/tmp/sdcard/debug_log"
        );
    }

    #[test]
    fn ssh_devices_preserve_custom_default_log_path() {
        let mut draft = ssh_ready_draft();
        draft.ssh_default_log_path = "/var/log/custom.log".into();

        let device = draft.to_device().expect("ssh device");
        assert_eq!(
            device.ssh.expect("ssh config").default_log_path,
            "/var/log/custom.log"
        );
    }

    #[test]
    fn adb_only_devices_are_valid_and_default_to_serial_selector() {
        let draft = adb_ready_draft();

        let device = draft.to_device().expect("adb device");
        let adb = device.adb.expect("adb config");
        assert_eq!(adb.serial, "08211FDD4002KE");
        assert_eq!(adb.transport, None);
        assert_eq!(adb.default_logcat_args, vec!["-v", "time"]);
        assert!(device.ssh.is_none());
        assert!(device.serial.is_none());
        assert!(device.jlink.is_none());
    }

    #[test]
    fn adb_logcat_args_default_when_field_is_blank() {
        let mut draft = adb_ready_draft();
        draft.adb_default_logcat_args.clear();

        let device = draft.to_device().expect("adb device");
        assert_eq!(
            device.adb.expect("adb config").default_logcat_args,
            vec!["-v", "time"]
        );
    }

    #[test]
    fn adb_usb_transport_is_preserved() {
        let mut draft = adb_ready_draft();
        draft.adb_transport = "usb".into();

        let device = draft.to_device().expect("adb usb device");
        assert_eq!(
            device.adb.expect("adb config").transport.as_deref(),
            Some("usb")
        );
    }
}
