use yew::prelude::*;
use yew_router::prelude::Link;

use crate::app::Route;

#[derive(Properties, PartialEq)]
pub struct FrameProps {
    #[prop_or_default]
    pub children: Children,
}

#[function_component(Frame)]
pub fn frame(props: &FrameProps) -> Html {
    html! {
        <main class="shell">
            <section class="hero">
                <span class="eyebrow">{"Local-first device control plane"}</span>
                <h1>{"Device Hub"}</h1>
                <p>{"Scaffolded around structured jobs, unified device events, and a Rust-only stack for server plus WASM UI."}</p>
            </section>
            <nav class="nav">
                <Link<Route> to={Route::Devices}>{"Devices"}</Link<Route>>
                <Link<Route> to={Route::LogViewer}>{"Live Logs"}</Link<Route>>
            </nav>
            { for props.children.iter() }
        </main>
    }
}
