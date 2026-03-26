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
        <section class="grid">
            <article class="card">
                <span class="eyebrow">{"Devices"}</span>
                <h2>{"Registered targets"}</h2>
                <p class="muted">{"Startup config is file-backed, so the UI reflects the same typed registry the server exposes to AI callers."}</p>
            </article>
            <article class="card">
                <span class="eyebrow">{"Status"}</span>
                <h2>{"Control plane ready"}</h2>
                <p class="muted">{"Next implementation steps can replace stub jobs with real SSH and J-Link execution paths."}</p>
            </article>
            <article class="card" style="grid-column: 1 / -1;">
                <div class="list">
                    {
                        if let Some(message) = (*error).clone() {
                            html! { <p>{message}</p> }
                        } else if let Some(list) = (*devices).clone() {
                            if list.is_empty() {
                                html! { <p>{"No devices loaded. Create `devices.yaml` from the example file to populate the registry."}</p> }
                            } else {
                                html! {
                                    <>
                                        { for list.into_iter().map(view_device) }
                                    </>
                                }
                            }
                        } else {
                            html! { <p>{"Loading devices..."}</p> }
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
        <div class="row">
            <div>
                <h3>{device.label.clone()}</h3>
                <p class="mono muted">{device.id.clone()}</p>
            </div>
            <div>
                <span class="status">{
                    match (device.ssh.is_some(), device.jlink.is_some()) {
                        (true, true) => "ssh + jlink",
                        (true, false) => "ssh",
                        (false, true) => "jlink",
                        (false, false) => "unconfigured",
                    }
                }</span>
                <span>{" "}</span>
                <Link<Route> to={route}>{"Open"}</Link<Route>>
            </div>
        </div>
    }
}
