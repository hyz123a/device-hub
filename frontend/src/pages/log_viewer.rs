use yew::prelude::*;

#[function_component(LogViewerPage)]
pub fn log_viewer_page() -> Html {
    html! {
        <section class="grid">
            <article class="card" style="grid-column: 1 / -1;">
                <span class="eyebrow">{"Live logs"}</span>
                <h2>{"WebSocket viewer scaffold"}</h2>
                <p class="muted">{"The route is in place on both client and server. The next step is to wire this page to `/ws` and stream filtered `LogEvent` frames into the timeline."}</p>
            </article>
        </section>
    }
}
