use reqwest::StatusCode;

use crate::helpers::spawn_app;

#[tokio::test]
async fn root_serves_embedded_frontend_index() {
    let app = spawn_app().await;

    let response = app
        .api_client
        .get(app.endpoint("/"))
        .send()
        .await
        .expect("failed to execute request");

    assert!(response.status().is_success());
    assert_eq!(
        response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("text/html; charset=utf-8")
    );
    let body = response.text().await.expect("read root body");
    assert!(body.contains("Device Hub"));
    assert!(body.contains("<title>Device Hub</title>"));
}

#[tokio::test]
async fn spa_route_returns_embedded_index() {
    let app = spawn_app().await;

    let response = app
        .api_client
        .get(app.endpoint("/devices/board-alpha"))
        .send()
        .await
        .expect("failed to execute request");

    assert!(response.status().is_success());
    let body = response.text().await.expect("read spa body");
    assert!(body.contains("<title>Device Hub</title>"));
}

#[tokio::test]
async fn embedded_asset_request_returns_expected_bytes() {
    let app = spawn_app().await;
    let root = app
        .api_client
        .get(app.endpoint("/"))
        .send()
        .await
        .expect("failed to fetch root html")
        .text()
        .await
        .expect("read root html");
    let asset_path = extract_asset_path(&root).expect("root html should reference a js asset");

    let response = app
        .api_client
        .get(app.endpoint(&asset_path))
        .send()
        .await
        .expect("failed to execute request");

    assert!(response.status().is_success());
    assert_eq!(
        response
            .headers()
            .get(reqwest::header::CACHE_CONTROL)
            .and_then(|value| value.to_str().ok()),
        Some("public, max-age=31536000, immutable")
    );
    let body = response.text().await.expect("read asset body");
    assert!(!body.is_empty());
}

#[tokio::test]
async fn missing_asset_returns_not_found() {
    let app = spawn_app().await;

    let response = app
        .api_client
        .get(app.endpoint("/assets/missing.js"))
        .send()
        .await
        .expect("failed to execute request");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

fn extract_asset_path(html: &str) -> Option<String> {
    let script_attr = "src=\"/";
    if let Some(start) = html.find(script_attr) {
        let path_start = start + "src=\"".len();
        let path_end = html[path_start..].find('"')? + path_start;
        return Some(html[path_start..path_end].to_string());
    }

    let import_attr = "import init, * as bindings from '/";
    if let Some(start) = html.find(import_attr) {
        let path_start = start + "import init, * as bindings from '".len();
        let path_end = html[path_start..].find('\'')? + path_start;
        return Some(html[path_start..path_end].to_string());
    }

    None
}
