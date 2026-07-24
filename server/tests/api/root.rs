use serde_json::json;

use crate::helpers::{spawn_app, spawn_app_with_config, test_config};
use device_hub_shared::CapabilitySnapshot;

#[tokio::test]
async fn root_returns_service_metadata() {
    let app = spawn_app().await;

    let response = app
        .api_client
        .get(app.endpoint("/api"))
        .send()
        .await
        .expect("failed to execute request");

    assert!(response.status().is_success());
    let body: serde_json::Value = response.json().await.expect("invalid json body");
    assert_eq!(body, json!({"name": "device-hub", "status": "ok"}));
}

#[tokio::test]
async fn capabilities_report_compiled_enabled_and_available_adapters() {
    let app = spawn_app().await;

    let response = app
        .api_client
        .get(app.endpoint("/api/capabilities"))
        .send()
        .await
        .expect("failed to execute request");

    assert!(response.status().is_success());
    let snapshot: CapabilitySnapshot = response.json().await.expect("invalid json body");
    for adapter_id in ["serial", "ssh", "jlink", "adb"] {
        let capability = snapshot.adapter(adapter_id).expect("missing capability");
        assert!(capability.compiled);
        assert!(capability.enabled);
        assert!(capability.available);
        assert_eq!(capability.unavailable_reason, None);
    }
}

#[tokio::test]
async fn disabled_adapter_is_not_registered_or_available() {
    let mut config = test_config();
    config.adapters.ssh = false;
    let app = spawn_app_with_config(config).await;

    let snapshot: CapabilitySnapshot = app
        .api_client
        .get(app.endpoint("/api/capabilities"))
        .send()
        .await
        .expect("failed to execute request")
        .json()
        .await
        .expect("invalid capability json");
    let ssh = snapshot.adapter("ssh").expect("missing ssh capability");
    assert!(ssh.compiled);
    assert!(!ssh.enabled);
    assert!(!ssh.available);
    assert_eq!(ssh.unavailable_reason.as_deref(), Some("disabled"));

    let response = app
        .post_action(
            "board-alpha",
            "ssh.exec",
            Some(&device_hub_shared::ActionRequest {
                payload: Some("true".into()),
            }),
        )
        .await;
    assert_eq!(response.status(), reqwest::StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        response.text().await.expect("error body"),
        r#"{"error":"capability unavailable: action `ssh.exec` (disabled)"}"#
    );
}
