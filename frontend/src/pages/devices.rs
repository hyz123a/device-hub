use wasm_bindgen_futures::spawn_local;
use yew::prelude::*;
use yew_router::prelude::Link;

use crate::{api, app::Route};
use device_hub_shared::Device;

#[function_component(DevicesPage)]
pub fn devices_page() -> Html {
    let devices = use_state(|| Option::<Vec<Device>>::None);
    let error = use_state(|| Option::<String>::None);

    {
        let devices = devices.clone();
        let error = error.clone();
        use_effect_with((), move |_| {
            spawn_local(async move {
                match api::list_devices().await {
                    Ok(list) => devices.set(Some(list)),
                    Err(err) => error.set(Some(err)),
                }
            });
            || {}
        });
    }

    html! {
        <section class="grid gap-5 xl:grid-cols-2">
            <article class="rounded-[24px] border border-black/10 bg-white/80 p-6 shadow-shell backdrop-blur">
                <span class="inline-flex items-center gap-2 uppercase tracking-[0.18em] text-[0.74rem] text-accent-strong">{"Devices"}</span>
                <h2 class="mb-3 mt-3 text-2xl font-semibold tracking-tight text-ink">{"Registered targets"}</h2>
                <p class="m-0 text-ink-muted">{"Startup config is file-backed, so the UI reflects the same typed registry the server exposes to AI callers."}</p>
            </article>
            <article class="rounded-[24px] border border-black/10 bg-white/80 p-6 shadow-shell backdrop-blur">
                <span class="inline-flex items-center gap-2 uppercase tracking-[0.18em] text-[0.74rem] text-accent-strong">{"Status"}</span>
                <h2 class="mb-3 mt-3 text-2xl font-semibold tracking-tight text-ink">{"Control plane ready"}</h2>
                <p class="m-0 text-ink-muted">{"SSH actions, live logs, and persisted job artifacts are already wired through the same local control plane the API exposes to AI callers."}</p>
            </article>
            <article class="rounded-[24px] border border-black/10 bg-white/80 p-6 shadow-shell backdrop-blur xl:col-span-2">
                <div class="grid gap-4">
                    {
                        if let Some(message) = (*error).clone() {
                            html! { <p class="m-0 text-sm text-red-700">{message}</p> }
                        } else if let Some(list) = (*devices).clone() {
                            if list.is_empty() {
                                html! { <p class="m-0 text-ink-muted">{"No devices loaded. Create `devices.yaml` from the example file to populate the registry."}</p> }
                            } else {
                                html! {
                                    <>
                                        { for list.into_iter().map(view_device) }
                                    </>
                                }
                            }
                        } else {
                            html! { <p class="m-0 text-ink-muted">{"Loading devices..."}</p> }
                        }
                    }
                </div>
            </article>
        </section>
    }
}

fn view_device(device: Device) -> Html {
    let route = Route::DeviceDetail {
        device_id: device.id.clone(),
    };

    html! {
        <div class="flex flex-col gap-3 border-t border-black/10 pt-3 first:border-t-0 first:pt-0 sm:flex-row sm:items-center sm:justify-between">
            <div>
                <h3 class="mb-1 mt-0 text-lg font-semibold text-ink">{device.label.clone()}</h3>
                <p class="m-0 font-mono text-sm text-ink-muted">{device.id.clone()}</p>
            </div>
            <div class="flex flex-wrap items-center gap-3">
                <span class="inline-flex rounded-full bg-accent/10 px-3 py-1.5 text-xs font-medium text-accent-strong">{
                    match (device.ssh.is_some(), device.jlink.is_some()) {
                        (true, true) => "ssh + jlink",
                        (true, false) => "ssh",
                        (false, true) => "jlink",
                        (false, false) => "unconfigured",
                    }
                }</span>
                <Link<Route>
                    classes="text-sm font-medium text-accent-strong transition hover:text-accent"
                    to={route}
                >{"Open"}</Link<Route>>
            </div>
        </div>
    }
}
