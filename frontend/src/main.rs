mod api;
mod app;
mod components;
mod pages;

fn main() {
    yew::Renderer::<app::App>::new().render();
}
