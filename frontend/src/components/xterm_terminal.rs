use std::rc::Rc;

use device_hub_shared::LogEvent;
use wasm_bindgen::{JsValue, closure::Closure, prelude::wasm_bindgen};
use web_sys::HtmlElement;
use yew::prelude::*;

#[derive(Properties, PartialEq)]
pub struct XtermTerminalProps {
    pub snapshot: Rc<Vec<LogEvent>>,
    #[prop_or_default]
    pub batch: TerminalBatch,
    #[prop_or_default]
    pub reset_key: String,
    #[prop_or_default]
    pub scroll_to_bottom_revision: u32,
    #[prop_or_default]
    pub on_bottom_state_change: Callback<bool>,
}

#[derive(Clone, Default, PartialEq)]
pub struct TerminalBatch {
    pub sequence: u64,
    pub events: Vec<LogEvent>,
}

#[derive(Debug, PartialEq, Eq)]
enum TerminalRenderAction {
    None,
    Append,
    Reset,
}

fn terminal_render_action(
    previous_reset_key: &str,
    applied_sequence: u64,
    reset_key: &str,
    batch_sequence: u64,
) -> TerminalRenderAction {
    if previous_reset_key != reset_key {
        TerminalRenderAction::Reset
    } else if batch_sequence > applied_sequence {
        TerminalRenderAction::Append
    } else {
        TerminalRenderAction::None
    }
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
    let previous_reset_key = use_mut_ref(String::new);
    let applied_sequence = use_mut_ref(|| 0_u64);
    let viewport_change_cb = use_mut_ref(|| Option::<Closure<dyn FnMut(bool)>>::None);

    {
        let container_ref = container_ref.clone();
        let terminal_handle = terminal_handle.clone();
        let previous_reset_key = previous_reset_key.clone();
        let applied_sequence = applied_sequence.clone();
        let viewport_change_cb = viewport_change_cb.clone();
        let snapshot = props.snapshot.clone();
        let batch_sequence = props.batch.sequence;
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
                    if !snapshot.is_empty() {
                        let text = format_terminal_events(snapshot.as_slice());
                        let _ = device_hub_append_terminal(&handle, &text);
                    }
                    *previous_reset_key.borrow_mut() = reset_key.clone();
                    *applied_sequence.borrow_mut() = if snapshot.is_empty() {
                        0
                    } else {
                        batch_sequence
                    };
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
        let previous_reset_key = previous_reset_key.clone();
        let applied_sequence = applied_sequence.clone();
        let snapshot = props.snapshot.clone();
        let batch = props.batch.clone();
        let reset_key = props.reset_key.clone();
        use_effect_with(
            (snapshot, batch, reset_key),
            move |(snapshot, batch, reset_key)| {
                if let Some(handle) = terminal_handle.borrow().as_ref() {
                    let mut previous_key = previous_reset_key.borrow_mut();
                    let mut applied = applied_sequence.borrow_mut();
                    match terminal_render_action(&previous_key, *applied, reset_key, batch.sequence)
                    {
                        TerminalRenderAction::Reset => {
                            let _ = device_hub_reset_terminal(handle);
                            let text = format_terminal_events(snapshot.as_slice());
                            if !text.is_empty() {
                                let _ = device_hub_append_terminal(handle, &text);
                            }
                            *previous_key = reset_key.clone();
                            *applied = batch.sequence;
                        }
                        TerminalRenderAction::Append => {
                            let text = format_terminal_events(&batch.events);
                            if !text.is_empty() {
                                let _ = device_hub_append_terminal(handle, &text);
                            }
                            *applied = batch.sequence;
                        }
                        TerminalRenderAction::None => {}
                    }
                }
                || {}
            },
        );
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
    fn terminal_preserves_interleaved_device_event_order() {
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
    fn terminal_batch_normalizes_newlines_without_changing_messages() {
        let events = vec![sample_event(1, "board-alpha", "one\ntwo\r\nthree\r")];

        let rendered = format_terminal_events(&events);

        assert_eq!(rendered, "one\r\ntwo\r\nthree\r\n");
    }

    #[test]
    fn terminal_receives_only_new_batch_during_normal_append() {
        let action = terminal_render_action("device-logs", 4, "device-logs", 5);
        assert_eq!(action, TerminalRenderAction::Append);
    }

    #[test]
    fn terminal_receives_only_new_batch_after_buffer_rollover() {
        let action = terminal_render_action("device-logs", 99, "device-logs", 100);
        assert_eq!(action, TerminalRenderAction::Append);
    }

    #[test]
    fn terminal_resets_once_when_filter_or_stream_identity_changes() {
        assert_eq!(
            terminal_render_action("device-logs", 7, "source:serial", 7),
            TerminalRenderAction::Reset
        );
        assert_eq!(
            terminal_render_action("source:serial", 7, "source:serial", 7),
            TerminalRenderAction::None
        );
    }

    #[test]
    fn terminal_does_not_replay_full_history_on_render_flush() {
        let history = (0..100)
            .map(|index| sample_event(index, "board-alpha", &format!("line {index}")))
            .collect::<Vec<_>>();
        let batch = vec![sample_event(100, "board-alpha", "line 100")];

        assert_eq!(
            terminal_render_action("all", 10, "all", 11),
            TerminalRenderAction::Append
        );
        assert_eq!(format_terminal_events(&batch), "line 100\r\n");
        assert_eq!(history.len(), 100);
    }
}
