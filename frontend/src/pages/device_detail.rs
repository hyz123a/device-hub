use wasm_bindgen_futures::spawn_local;
use yew::prelude::*;

use crate::api;
use device_hub_shared::DeviceDetail;

#[derive(Properties, PartialEq)]
pub struct DeviceDetailProps {
    pub device_id: String,
}

#[function_component(DeviceDetailPage)]
pub fn device_detail_page(props: &DeviceDetailProps) -> Html {
    let detail = use_state(|| Option::<DeviceDetail>::None);
    let error = use_state(|| Option::<String>::None);

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
        <section class="grid">
            <article class="card" style="grid-column: 1 / -1;">
                <span class="eyebrow">{"Device detail"}</span>
                {
                    if let Some(message) = (*error).clone() {
                        html! { <p>{message}</p> }
                    } else if let Some(detail) = (*detail).clone() {
                        html! {
                            <>
                                <h2>{detail.device.label}</h2>
                                <p class="mono muted">{detail.device.id}</p>
                                <div class="row">
                                    <div>
                                        <h3>{"SSH"}</h3>
                                        <p>{if detail.capabilities.has_ssh { "Configured" } else { "Unavailable" }}</p>
                                    </div>
                                    <div>
                                        <h3>{"J-Link"}</h3>
                                        <p>{if detail.capabilities.has_jlink { "Configured" } else { "Unavailable" }}</p>
                                    </div>
                                </div>
                            </>
                        }
                    } else {
                        html! { <p>{"Loading device..."}</p> }
                    }
                }
            </article>
        </section>
    }
}
