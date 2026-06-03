use device_hub_shared::LogEvent;
use wasm_bindgen::{JsValue, closure::Closure, prelude::wasm_bindgen};
use web_sys::HtmlElement;
use yew::prelude::*;

#[derive(Properties, PartialEq)]
pub struct XtermTerminalProps {
    pub events: Vec<LogEvent>,
    #[prop_or_default]
    pub scroll_to_bottom_revision: u32,
    #[prop_or_default]
    pub on_bottom_state_change: Callback<bool>,
    #[prop_or_default]
    pub logs_fullscreen: bool,
}

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(catch, js_namespace = window, js_name = deviceHubCreateTerminal)]
    fn device_hub_create_terminal(container: HtmlElement) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(catch, js_namespace = window, js_name = deviceHubCreateTerminalWithViewportHandler)]
    fn device_hub_create_terminal_with_viewport_handler(
        container: HtmlElement,
        on_viewport_change: &JsValue,
    ) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(catch, js_namespace = window, js_name = deviceHubAppendTerminal)]
    fn device_hub_append_terminal(handle: &JsValue, text: &str) -> Result<(), JsValue>;

    #[wasm_bindgen(catch, js_namespace = window, js_name = deviceHubResetTerminal)]
    fn device_hub_reset_terminal(handle: &JsValue) -> Result<(), JsValue>;

    #[wasm_bindgen(catch, js_namespace = window, js_name = deviceHubScrollToBottom)]
    fn device_hub_scroll_to_bottom(handle: &JsValue) -> Result<(), JsValue>;

    #[wasm_bindgen(catch, js_namespace = window, js_name = deviceHubFitTerminal)]
    fn device_hub_fit_terminal(handle: &JsValue) -> Result<(), JsValue>;

    #[wasm_bindgen(catch, js_namespace = window, js_name = deviceHubDisposeTerminal)]
    fn device_hub_dispose_terminal(handle: &JsValue) -> Result<(), JsValue>;
}

#[function_component(XtermTerminal)]
pub fn xterm_terminal(props: &XtermTerminalProps) -> Html {
    let container_ref = use_node_ref();
    let terminal_handle = use_mut_ref(|| Option::<JsValue>::None);
    let previous_events = use_mut_ref(Vec::<LogEvent>::new);
    let viewport_change_cb = use_mut_ref(|| Option::<Closure<dyn FnMut(bool)>>::None);

    {
        let container_ref = container_ref.clone();
        let terminal_handle = terminal_handle.clone();
        let previous_events = previous_events.clone();
        let viewport_change_cb = viewport_change_cb.clone();
        let events = props.events.clone();
        let on_bottom_state_change = props.on_bottom_state_change.clone();
        use_effect_with((), move |_| {
            if let Some(container) = container_ref.cast::<HtmlElement>() {
                let on_viewport_change = {
                    let on_bottom_state_change = on_bottom_state_change.clone();
                    Closure::wrap(Box::new(move |at_bottom: bool| {
                        on_bottom_state_change.emit(at_bottom);
                    }) as Box<dyn FnMut(bool)>)
                };
                let callback_js: JsValue = on_viewport_change.as_ref().clone();

                if let Ok(handle) =
                    device_hub_create_terminal_with_viewport_handler(container, &callback_js)
                {
                    if !events.is_empty() {
                        let snapshot = format_terminal_events(&events);
                        let _ = device_hub_append_terminal(&handle, &snapshot);
                        *previous_events.borrow_mut() = events.clone();
                    }
                    *terminal_handle.borrow_mut() = Some(handle);
                    *viewport_change_cb.borrow_mut() = Some(on_viewport_change);
                }
            }

            move || {
                if let Some(handle) = terminal_handle.borrow_mut().take() {
                    let _ = device_hub_dispose_terminal(&handle);
                }
                viewport_change_cb.borrow_mut().take();
            }
        });
    }

    {
        let terminal_handle = terminal_handle.clone();
        let previous_events = previous_events.clone();
        let events = props.events.clone();
        use_effect_with(events, move |events| {
            if let Some(handle) = terminal_handle.borrow().as_ref() {
                let mut previous = previous_events.borrow_mut();
                if events.is_empty() {
                    let _ = device_hub_reset_terminal(handle);
                    previous.clear();
                } else if previous.is_empty() || events.starts_with(previous.as_slice()) {
                    let appended = format_terminal_events(&events[previous.len()..]);
                    if !appended.is_empty() {
                        let _ = device_hub_append_terminal(handle, &appended);
                    }
                    *previous = events.clone();
                } else {
                    let snapshot = format_terminal_events(events);
                    let _ = device_hub_reset_terminal(handle);
                    if !snapshot.is_empty() {
                        let _ = device_hub_append_terminal(handle, &snapshot);
                    }
                    *previous = events.clone();
                }
            }
            || {}
        });
    }

    {
        let terminal_handle = terminal_handle.clone();
        use_effect_with(props.logs_fullscreen, move |_| {
            if let Some(handle) = terminal_handle.borrow().as_ref() {
                let _ = device_hub_fit_terminal(handle);
            }
            || {}
        });
    }

    {
        let terminal_handle = terminal_handle.clone();
        use_effect_with(props.scroll_to_bottom_revision, move |_| {
            if let Some(handle) = terminal_handle.borrow().as_ref() {
                let _ = device_hub_scroll_to_bottom(handle);
            }
            || {}
        });
    }

    html! {
        <div
            ref={container_ref}
            class={classes!(
                "xterm-shell",
                "min-w-0",
                "px-4",
                "py-4",
                if props.logs_fullscreen {
                    Some("max-h-[calc(100vh-14rem)]")
                } else {
                    Some("max-h-[70vh]")
                }
            )}
        />
    }
}

fn format_terminal_events(events: &[LogEvent]) -> String {
    events
        .iter()
        .map(|event| format_raw_terminal_event(&event.message))
        .collect()
}

fn format_raw_terminal_event(message: &str) -> String {
    let mut raw = message.replace("\r\n", "\n").replace('\r', "\n");
    if !raw.ends_with('\n') {
        raw.push('\n');
    }
    raw.replace('\n', "\r\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use device_hub_shared::{LogChannel, LogLevel, LogSource};

    fn sample_event(index: i64, device_id: &str, message: &str) -> LogEvent {
        LogEvent {
            ts: Utc
                .timestamp_opt(index, 0)
                .single()
                .expect("valid timestamp"),
            device_id: device_id.into(),
            job_id: "job-1".into(),
            source: LogSource::Serial,
            channel: LogChannel::Stdout,
            level: LogLevel::Info,
            message: message.into(),
        }
    }

    #[test]
    fn terminal_stream_preserves_event_order_without_device_headers() {
        let events = vec![
            sample_event(1, "board-alpha", "line one\n"),
            sample_event(2, "board-beta", "line two\n"),
        ];

        let rendered = format_terminal_events(&events);

        assert!(rendered.contains("line one\r\nline two\r\n"));
        assert!(!rendered.contains("board-alpha"));
        assert!(!rendered.contains("board-beta"));
    }

    #[test]
    fn terminal_stream_adds_missing_newline_per_event() {
        let events = vec![sample_event(1, "board-alpha", "hello")];

        let rendered = format_terminal_events(&events);

        assert_eq!(rendered, "hello\r\n");
    }

    #[test]
    fn terminal_stream_normalizes_existing_newlines_to_crlf() {
        let events = vec![sample_event(1, "board-alpha", "one\ntwo\r\nthree\r")];

        let rendered = format_terminal_events(&events);

        assert_eq!(rendered, "one\r\ntwo\r\nthree\r\n");
    }
}
