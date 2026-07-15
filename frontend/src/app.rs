use yew::prelude::*;
use yew_router::prelude::*;

use crate::{components::frame::Frame, pages};

const THEME_STORAGE_KEY: &str = "deviceHubTheme";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ThemeMode {
    Dracula,
    Light,
}

impl ThemeMode {
    fn from_storage(value: Option<String>) -> Self {
        match value.as_deref() {
            Some("light") => Self::Light,
            _ => Self::Dracula,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Dracula => "dracula",
            Self::Light => "light",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Dracula => "Dracula",
            Self::Light => "Light",
        }
    }

    fn next(self) -> Self {
        match self {
            Self::Dracula => Self::Light,
            Self::Light => Self::Dracula,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Routable)]
pub enum Route {
    #[at("/")]
    Devices,
    #[at("/devices/:device_id")]
    DeviceDetail { device_id: String },
    #[at("/devices/:device_id/logs")]
    DeviceLogs { device_id: String },
    #[at("/jobs/:job_id")]
    JobDetail { job_id: String },
    #[at("/logs")]
    LogViewer,
    #[not_found]
    #[at("/404")]
    NotFound,
}

fn switch(route: Route) -> Html {
    match route {
        Route::Devices => html! { <pages::devices::DevicesPage /> },
        Route::DeviceDetail { device_id } => {
            html! { <pages::device_detail::DeviceDetailPage device_id={device_id} /> }
        }
        Route::DeviceLogs { device_id } => {
            html! { <pages::log_viewer::LogViewerPage device_id={Some(device_id)} /> }
        }
        Route::JobDetail { job_id } => {
            html! { <pages::job_detail::JobDetailPage job_id={job_id} /> }
        }
        Route::LogViewer => {
            html! { <pages::log_viewer::LogViewerPage device_id={None::<String>} /> }
        }
        Route::NotFound => html! {
            <div class="dh-panel rounded-[24px] p-6">
                <h2 class="m-0 text-2xl font-semibold tracking-tight text-ink">{"Page not found"}</h2>
            </div>
        },
    }
}

#[function_component(App)]
pub fn app() -> Html {
    let theme = use_state(load_initial_theme);

    {
        let active_theme = *theme;
        use_effect_with(active_theme, move |active_theme| {
            apply_theme(*active_theme);
            || {}
        });
    }

    let on_toggle_theme = {
        let theme = theme.clone();
        Callback::from(move |_| theme.set(theme.next()))
    };

    html! {
        <BrowserRouter>
            <Frame theme_label={theme.label()} {on_toggle_theme}>
                <Switch<Route> render={switch} />
            </Frame>
        </BrowserRouter>
    }
}

fn load_initial_theme() -> ThemeMode {
    let stored = web_sys::window()
        .and_then(|window| window.local_storage().ok().flatten())
        .and_then(|storage| storage.get_item(THEME_STORAGE_KEY).ok().flatten());
    ThemeMode::from_storage(stored)
}

fn apply_theme(theme: ThemeMode) {
    if let Some(window) = web_sys::window() {
        if let Some(document) = window.document()
            && let Some(root) = document.document_element()
        {
            let _ = root.set_attribute("data-theme", theme.as_str());
        }

        if let Ok(Some(storage)) = window.local_storage() {
            let _ = storage.set_item(THEME_STORAGE_KEY, theme.as_str());
        }
    }
}
