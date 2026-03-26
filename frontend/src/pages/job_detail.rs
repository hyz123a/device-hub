use wasm_bindgen_futures::spawn_local;
use yew::prelude::*;

use crate::api;
use device_hub_shared::{Job, LogEvent};

#[derive(Properties, PartialEq)]
pub struct JobDetailProps {
    pub job_id: String,
}

#[function_component(JobDetailPage)]
pub fn job_detail_page(props: &JobDetailProps) -> Html {
    let job = use_state(|| Option::<Job>::None);
    let events = use_state(Vec::<LogEvent>::new);
    let error = use_state(|| Option::<String>::None);

    {
        let job = job.clone();
        let events = events.clone();
        let error = error.clone();
        let job_id = props.job_id.clone();
        use_effect_with(props.job_id.clone(), move |_| {
            spawn_local(async move {
                match api::get_job(&job_id).await {
                    Ok(value) => job.set(Some(value)),
                    Err(err) => error.set(Some(err.clone())),
                }
                if let Ok(value) = api::get_job_events(&job_id).await {
                    events.set(value);
                }
            });
            || {}
        });
    }

    html! {
        <section class="grid">
            <article class="card">
                <span class="eyebrow">{"Job"}</span>
                {
                    if let Some(message) = (*error).clone() {
                        html! { <p>{message}</p> }
                    } else if let Some(job) = (*job).clone() {
                        html! {
                            <>
                                <h2>{job.action}</h2>
                                <p class="mono muted">{job.id}</p>
                                <span class="status">{format!("{:?}", job.status)}</span>
                            </>
                        }
                    } else {
                        html! { <p>{"Loading job..."}</p> }
                    }
                }
            </article>
            <article class="card">
                <span class="eyebrow">{"Events"}</span>
                <div class="list">
                    { for (*events).iter().map(view_event) }
                </div>
            </article>
        </section>
    }
}

fn view_event(event: &LogEvent) -> Html {
    html! {
        <div class="row">
            <div>
                <strong>{event.message.clone()}</strong>
                <p class="mono muted">{event.ts.to_rfc3339()}</p>
            </div>
            <span class="status">{format!("{:?}", event.level)}</span>
        </div>
    }
}
