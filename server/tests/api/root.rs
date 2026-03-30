use serde_json::json;

use crate::helpers::spawn_app;

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
