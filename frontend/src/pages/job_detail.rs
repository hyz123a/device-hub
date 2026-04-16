use device_hub_shared::{Job, JobArtifact, JobEvidence, JobStatus, LogEvent};
use wasm_bindgen_futures::spawn_local;
use yew::prelude::*;
use yew_router::prelude::*;

use crate::components::stream_log_panel::StreamLogPanel;
use crate::{api, app::Route};

#[derive(Properties, PartialEq)]
pub struct JobDetailProps {
    pub job_id: String,
}

#[function_component(JobDetailPage)]
pub fn job_detail_page(props: &JobDetailProps) -> Html {
    let job = use_state(|| Option::<Job>::None);
    let evidence = use_state(|| Option::<JobEvidence>::None);
    let initial_events = use_state(Vec::<LogEvent>::new);
    let error = use_state(|| Option::<String>::None);
    let connection = use_state(|| "connecting".to_owned());
    let rerun_pending = use_state(|| false);
    let ws_url = use_state(|| Option::<String>::None);
    let connect_revision = use_state(|| 0_u32);
    let navigator = use_navigator();

    // Fetch job evidence + historical events, then open WS
    {
        let job = job.clone();
        let evidence = evidence.clone();
        let initial_events = initial_events.clone();
        let error = error.clone();
        let connection = connection.clone();
        let ws_url = ws_url.clone();
        let connect_revision = connect_revision.clone();
        let job_id = props.job_id.clone();
        use_effect_with(props.job_id.clone(), move |_| {
            job.set(None);
            evidence.set(None);
            initial_events.set(Vec::new());
            error.set(None);
            connection.set("connecting".to_owned());
            ws_url.set(None);
            connect_revision.set(connect_revision.wrapping_add(1));

            let job = job.clone();
            let evidence = evidence.clone();
            let initial_events = initial_events.clone();
            let error = error.clone();
            let connection = connection.clone();
            let ws_url = ws_url.clone();
            spawn_local(async move {
                match api::get_job_evidence(&job_id).await {
                    Ok(value) => {
                        job.set(Some(value.job.clone()));
                        evidence.set(Some(value));
                    }
                    Err(err) => {
                        error.set(Some(err));
                        connection.set("disconnected".to_owned());
                        return;
                    }
                }

                match api::get_job_events(&job_id).await {
                    Ok(events) => {
                        initial_events.set(events);
                    }
                    Err(err) => {
                        error.set(Some(err));
                        connection.set("disconnected".to_owned());
                        return;
                    }
                }

                match api::ws_url(None, Some(&job_id)) {
                    Ok(url) => ws_url.set(Some(url)),
                    Err(msg) => {
                        connection.set("disconnected".to_owned());
                        error.set(Some(msg));
                    }
                }
            });

            || {}
        });
    }

    let on_connection_change = {
        let connection = connection.clone();
        Callback::from(move |status: String| connection.set(status))
    };
    let on_ws_error = {
        let error = error.clone();
        Callback::from(move |msg: String| error.set(Some(msg)))
    };

    let status_class = match connection.as_str() {
        "connected" => "inline-flex rounded-full bg-accent/10 px-3 py-1.5 text-xs font-medium text-accent-strong",
        "connecting" => "inline-flex rounded-full bg-amber-500/15 px-3 py-1.5 text-xs font-medium text-amber-800",
        _ => "inline-flex rounded-full bg-black/5 px-3 py-1.5 text-xs font-medium text-ink-muted",
    };

    let job_evidence = (*evidence).clone();
    let serial_write_device_id = job
        .as_ref()
        .as_ref()
        .map(|j| j.device_id.clone())
        .filter(|v| !v.is_empty());

    let on_rerun = {
        let navigator = navigator.clone();
        let rerun_pending = rerun_pending.clone();
        let error = error.clone();
        let job_id = props.job_id.clone();
        Callback::from(move |_| {
            let navigator = navigator.clone();
            let rerun_pending = rerun_pending.clone();
            let error = error.clone();
            let job_id = job_id.clone();
            rerun_pending.set(true);
            spawn_local(async move {
                match api::rerun_job(&job_id).await {
                    Ok(new_job) => {
                        rerun_pending.set(false);
                        error.set(None);
                        if let Some(nav) = navigator {
                            nav.push(&Route::JobDetail {
                                job_id: new_job.id,
                            });
                        }
                    }
                    Err(message) => {
                        rerun_pending.set(false);
                        error.set(Some(message));
                    }
                }
            });
        })
    };

    html! {
        <section class="grid gap-5">
            <article class="min-w-0 rounded-[24px] border border-black/10 bg-white/80 p-6 shadow-shell backdrop-blur">
                <span class="inline-flex items-center gap-2 uppercase tracking-[0.18em] text-[0.74rem] text-accent-strong">{"Job"}</span>
                {
                    if let Some(message) = (*error).clone() {
                        html! { <p class="mb-0 mt-4 text-sm text-red-700">{message}</p> }
                    } else if let Some(job) = (*job).clone() {
                        let job_id = job.id.clone();
                        html! {
                            <>
                                <h2 class="mb-2 mt-3 text-3xl font-semibold tracking-tight text-ink">{job.action}</h2>
                                <p class="m-0 font-mono text-sm text-ink-muted">{job_id.clone()}</p>
                                <div class="mt-4 flex flex-wrap items-center gap-3">
                                    <span class={job_status_class(&job.status)}>{format!("{:?}", job.status)}</span>
                                    <span class={status_class}>{(*connection).clone()}</span>
                                    <span class="inline-flex rounded-full bg-black/5 px-3 py-1.5 font-mono text-xs text-ink-muted">{format!("timeout {}s", job.timeout_secs)}</span>
                                </div>
                                <div class="mt-4 flex flex-wrap gap-3">
                                    <button
                                        class="rounded-2xl border border-black/10 bg-white/75 px-4 py-3 text-sm font-medium text-ink transition hover:bg-white disabled:cursor-default disabled:opacity-55"
                                        onclick={on_rerun.clone()}
                                        disabled={*rerun_pending}
                                    >{if *rerun_pending { "Rerunning..." } else { "Rerun with same inputs" }}</button>
                                </div>
                                {
                                    if let Some(evidence) = job_evidence.clone() {
                                        html! {
                                            <div class="mt-4 grid gap-4 xl:grid-cols-[minmax(0,1.2fr)_minmax(18rem,0.8fr)]">
                                                <div class="rounded-[20px] border border-black/10 bg-black/[0.02] p-4">
                                                    <h3 class="m-0 text-lg font-semibold text-ink">{"Evidence summary"}</h3>
                                                    <p class="mb-0 mt-3 text-sm text-ink-muted">{evidence.summary.headline.clone()}</p>
                                                    {
                                                        if let Some(request) = evidence.summary.request.clone() {
                                                            html! { <p class="mb-0 mt-2 font-mono text-sm text-ink">{request}</p> }
                                                        } else { Html::default() }
                                                    }
                                                    {
                                                        if let Some(outcome) = evidence.summary.outcome.clone() {
                                                            html! { <p class="mb-0 mt-2 text-sm text-ink">{outcome}</p> }
                                                        } else { Html::default() }
                                                    }
                                                    <div class="mt-3 grid gap-2">
                                                        {for evidence.summary.highlights.iter().map(|line| html! {
                                                            <p class="m-0 text-sm text-ink-muted">{line}</p>
                                                        })}
                                                    </div>
                                                </div>
                                                <div class="rounded-[20px] border border-black/10 bg-black/[0.02] p-4">
                                                    <h3 class="m-0 text-lg font-semibold text-ink">{"Artifacts"}</h3>
                                                    <div class="mt-3 grid gap-2">
                                                        {render_artifacts(&evidence.artifacts)}
                                                    </div>
                                                </div>
                                            </div>
                                        }
                                    } else if is_terminal_status(&job.status) {
                                        html! {
                                            <p class="mb-0 mt-4 text-ink-muted">
                                                {"Loading persisted evidence summary and artifact metadata..."}
                                            </p>
                                        }
                                    } else {
                                        html! {
                                            <p class="mb-0 mt-4 text-ink-muted">
                                                {"Live events stream below. The runner persists job metadata and event history continuously, then exposes artifact downloads and a rerun entrypoint when you need another attempt."}
                                            </p>
                                        }
                                    }
                                }
                            </>
                        }
                    } else {
                        html! { <p class="mb-0 mt-4 text-ink-muted">{"Loading job..."}</p> }
                    }
                }
            </article>
            <StreamLogPanel
                initial_events={(*initial_events).clone()}
                ws_url={(*ws_url).clone()}
                connect_revision={*connect_revision}
                serial_write_device_id={serial_write_device_id}
                connection_status={Some((*connection).clone())}
                on_connection_change={on_connection_change}
                on_error={on_ws_error}
            />
        </section>
    }
}

fn job_status_class(status: &JobStatus) -> &'static str {
    match status {
        JobStatus::Succeeded => "inline-flex rounded-full bg-accent/10 px-3 py-1.5 text-xs font-medium text-accent-strong",
        JobStatus::Failed => "inline-flex rounded-full bg-red-700/10 px-3 py-1.5 text-xs font-medium text-red-700",
        JobStatus::Cancelled => "inline-flex rounded-full bg-black/5 px-3 py-1.5 text-xs font-medium text-ink-muted",
        JobStatus::Pending | JobStatus::Running => "inline-flex rounded-full bg-amber-500/15 px-3 py-1.5 text-xs font-medium text-amber-800",
    }
}

fn is_terminal_status(status: &JobStatus) -> bool {
    matches!(
        status,
        JobStatus::Succeeded | JobStatus::Failed | JobStatus::Cancelled
    )
}

fn render_artifacts(artifacts: &[JobArtifact]) -> Html {
    if artifacts.is_empty() {
        return html! { <p class="m-0 text-sm text-ink-muted">{"No artifacts persisted yet."}</p> };
    }
    html! {
        for artifacts.iter().map(|artifact| html! {
            <a
                class="flex items-center justify-between gap-3 rounded-2xl border border-black/10 bg-white/75 px-4 py-3 text-sm text-ink transition hover:bg-white"
                href={api::artifact_url(&artifact.download_url)}
                target="_blank"
                rel="noreferrer"
            >
                <span class="font-mono">{artifact.name.clone()}</span>
                <span class="text-ink-muted">{format_bytes(artifact.size_bytes)}</span>
            </a>
        })
    }
}

fn format_bytes(size_bytes: u64) -> String {
    if size_bytes >= 1024 * 1024 {
        format!("{:.1} MiB", size_bytes as f64 / (1024.0 * 1024.0))
    } else if size_bytes >= 1024 {
        format!("{:.1} KiB", size_bytes as f64 / 1024.0)
    } else {
        format!("{size_bytes} B")
    }
}
