use device_hub_shared::LogEvent;
use wasm_bindgen::{JsValue, closure::Closure, prelude::wasm_bindgen};
use web_sys::HtmlElement;
use yew::prelude::*;

#[derive(Properties, PartialEq)]
pub struct XtermTerminalProps {
    pub events: Vec<LogEvent>,
    #[prop_or_default]
    pub reset_key: String,
    #[prop_or_default]
    pub scroll_to_bottom_revision: u32,
    #[prop_or_default]
    pub on_bottom_state_change: Callback<bool>,
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
    let previous_reset_key = use_mut_ref(String::new);
    let viewport_change_cb = use_mut_ref(|| Option::<Closure<dyn FnMut(bool)>>::None);

    {
        let container_ref = container_ref.clone();
        let terminal_handle = terminal_handle.clone();
        let previous_events = previous_events.clone();
        let previous_reset_key = previous_reset_key.clone();
        let viewport_change_cb = viewport_change_cb.clone();
        let events = props.events.clone();
        let reset_key = props.reset_key.clone();
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
                    *previous_reset_key.borrow_mut() = reset_key.clone();
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
        let previous_reset_key = previous_reset_key.clone();
        let events = props.events.clone();
        let reset_key = props.reset_key.clone();
        use_effect_with((events, reset_key), move |(events, reset_key)| {
            if let Some(handle) = terminal_handle.borrow().as_ref() {
                let mut previous = previous_events.borrow_mut();
                let mut previous_key = previous_reset_key.borrow_mut();
                let force_reset = *previous_key != *reset_key;
                if events.is_empty() {
                    let _ = device_hub_reset_terminal(handle);
                    previous.clear();
                    *previous_key = reset_key.clone();
                } else if !force_reset {
                    if let Some(appended_events) =
                        continuing_appended_events(previous.as_slice(), events.as_slice())
                    {
                        let appended = format_terminal_events(appended_events);
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
                } else {
                    let snapshot = format_terminal_events(events);
                    let _ = device_hub_reset_terminal(handle);
                    if !snapshot.is_empty() {
                        let _ = device_hub_append_terminal(handle, &snapshot);
                    }
                    *previous = events.clone();
                    *previous_key = reset_key.clone();
                }
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
                "max-h-[70vh]"
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

fn continuing_appended_events<'a>(
    previous: &[LogEvent],
    events: &'a [LogEvent],
) -> Option<&'a [LogEvent]> {
    if previous.is_empty() || events.starts_with(previous) {
        return Some(&events[previous.len()..]);
    }

    if previous.len() != events.len() {
        return None;
    }

    let overlap = longest_suffix_prefix_overlap(previous, events);
    if overlap == 0 || overlap == events.len() {
        return None;
    }

    Some(&events[overlap..])
}

fn longest_suffix_prefix_overlap(previous: &[LogEvent], events: &[LogEvent]) -> usize {
    let max_overlap = previous.len().min(events.len());
    (1..=max_overlap)
        .rev()
        .find(|overlap| previous[previous.len() - overlap..] == events[..*overlap])
        .unwrap_or(0)
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

    #[test]
    fn continuing_events_append_after_prefix_growth() {
        let previous = vec![sample_event(1, "board-alpha", "one")];
        let events = vec![
            sample_event(1, "board-alpha", "one"),
            sample_event(2, "board-alpha", "two"),
        ];

        let appended = continuing_appended_events(&previous, &events).expect("append");

        assert_eq!(appended, &events[1..]);
    }

    #[test]
    fn continuing_events_append_after_sliding_window_rollover() {
        let previous = vec![
            sample_event(1, "board-alpha", "one"),
            sample_event(2, "board-alpha", "two"),
            sample_event(3, "board-alpha", "three"),
        ];
        let events = vec![
            sample_event(2, "board-alpha", "two"),
            sample_event(3, "board-alpha", "three"),
            sample_event(4, "board-alpha", "four"),
        ];

        let appended = continuing_appended_events(&previous, &events).expect("append");

        assert_eq!(appended, &events[2..]);
    }

    #[test]
    fn continuing_events_reject_non_contiguous_replacement() {
        let previous = vec![
            sample_event(1, "board-alpha", "one"),
            sample_event(2, "board-alpha", "two"),
        ];
        let events = vec![
            sample_event(10, "board-alpha", "ten"),
            sample_event(11, "board-alpha", "eleven"),
        ];

        assert!(continuing_appended_events(&previous, &events).is_none());
    }
}
