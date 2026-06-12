use yew::prelude::*;
use yew_router::prelude::Link;

use crate::app::Route;

#[derive(Properties, PartialEq)]
pub struct FrameProps {
    #[prop_or_default]
    pub children: Children,
    pub theme_label: AttrValue,
    pub on_toggle_theme: Callback<MouseEvent>,
}

#[function_component(Frame)]
pub fn frame(props: &FrameProps) -> Html {
    html! {
        <main class="mx-auto max-w-7xl px-4 pb-16 pt-8 sm:px-5 md:pb-20 md:pt-10">
            <section class="mb-6 grid gap-3">
                <span class="inline-flex items-center gap-2 uppercase tracking-[0.18em] text-[0.74rem] text-accent-strong">{"Local-first device control plane"}</span>
                <h1 class="m-0 max-w-4xl text-[clamp(2.5rem,4vw,4.6rem)] font-semibold leading-[0.94] tracking-[-0.05em] text-ink">{"Device Hub"}</h1>
                <p class="m-0 max-w-3xl text-[1.05rem] text-ink-muted">{"Scaffolded around structured jobs, unified device events, and a Rust-only stack for server plus WASM UI."}</p>
            </section>
            <nav class="mb-7 flex flex-wrap items-center gap-3">
                <Link<Route>
                    classes="dh-nav-link"
                    to={Route::Devices}
                >{"Devices"}</Link<Route>>
                <Link<Route>
                    classes="dh-nav-link"
                    to={Route::LogViewer}
                >{"Live Logs"}</Link<Route>>
                <button
                    class="dh-nav-link"
                    type="button"
                    onclick={props.on_toggle_theme.clone()}
                >{format!("Theme: {}", props.theme_label)}</button>
            </nav>
            { for props.children.iter() }
        </main>
    }
}
