use yew::prelude::*;
use yew_router::prelude::*;

use crate::{components::frame::Frame, pages};

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
            <div class="rounded-[24px] border border-black/10 bg-white/80 p-6 shadow-shell backdrop-blur">
                <h2 class="m-0 text-2xl font-semibold tracking-tight text-ink">{"Page not found"}</h2>
            </div>
        },
    }
}

#[function_component(App)]
pub fn app() -> Html {
    html! {
        <BrowserRouter>
            <Frame>
                <Switch<Route> render={switch} />
            </Frame>
        </BrowserRouter>
    }
}
