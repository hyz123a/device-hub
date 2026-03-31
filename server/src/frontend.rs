use std::{collections::HashMap, io::Cursor, sync::Arc};

use axum::{
    body::Body,
    extract::{Request, State},
    http::{HeaderValue, Method, Response, StatusCode, Uri, header},
    response::IntoResponse,
};
use thiserror::Error;

use crate::app::AppState;

#[derive(Clone, Debug)]
pub struct FrontendAsset {
    pub bytes: Vec<u8>,
    pub content_type: &'static str,
    pub cache_control: &'static str,
}

#[derive(Clone, Debug)]
pub struct FrontendAssets {
    inner: Arc<HashMap<String, FrontendAsset>>,
}

#[derive(Debug, Error)]
pub enum FrontendAssetError {
    #[error("failed to read embedded frontend bundle: {0}")]
    BundleRead(#[from] std::io::Error),
    #[error("embedded frontend bundle did not contain /index.html")]
    MissingIndex,
}

impl FrontendAssets {
    pub fn load_from_embedded_bundle() -> Result<Self, FrontendAssetError> {
        let bundle = include_bytes!(env!("DEVICE_HUB_FRONTEND_BUNDLE"));
        let mut archive = tar::Archive::new(Cursor::new(bundle.as_slice()));
        let mut assets = HashMap::new();

        for entry in archive.entries()? {
            let mut entry = entry?;
            if !entry.header().entry_type().is_file() {
                continue;
            }

            let path = normalize_archive_path(&entry.path()?);
            let mut bytes = Vec::new();
            std::io::Read::read_to_end(&mut entry, &mut bytes)?;
            assets.insert(
                path.clone(),
                FrontendAsset {
                    bytes,
                    content_type: content_type_for_path(&path),
                    cache_control: cache_control_for_path(&path),
                },
            );
        }

        if !assets.contains_key("/index.html") {
            return Err(FrontendAssetError::MissingIndex);
        }

        Ok(Self {
            inner: Arc::new(assets),
        })
    }

    pub fn get(&self, path: &str) -> Option<&FrontendAsset> {
        self.inner.get(path)
    }

    pub fn index(&self) -> Option<&FrontendAsset> {
        self.get("/index.html")
    }
}

pub async fn serve_embedded_asset(
    State(state): State<AppState>,
    uri: Uri,
    method: Method,
) -> Response<Body> {
    if method != Method::GET && method != Method::HEAD {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }

    let path = normalize_request_path(uri.path());
    let asset = if path == "/" {
        state.frontend.index()
    } else {
        match state.frontend.get(&path) {
            Some(asset) => Some(asset),
            None if !looks_like_asset_path(&path) && !path.starts_with("/api/") => {
                state.frontend.index()
            }
            None => None,
        }
    };

    match asset {
        Some(asset) => asset_response(asset, method == Method::HEAD),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

pub async fn api_not_found(_request: Request) -> impl IntoResponse {
    StatusCode::NOT_FOUND
}

fn asset_response(asset: &FrontendAsset, head_only: bool) -> Response<Body> {
    let mut response = if head_only {
        Response::new(Body::empty())
    } else {
        Response::new(Body::from(asset.bytes.clone()))
    };
    *response.status_mut() = StatusCode::OK;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(asset.content_type),
    );
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(asset.cache_control),
    );
    response
}

fn normalize_archive_path(path: &std::path::Path) -> String {
    let mut normalized = String::new();

    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::Normal(segment) => {
                normalized.push('/');
                normalized.push_str(&segment.to_string_lossy());
            }
            _ => {}
        }
    }

    if normalized.is_empty() {
        "/".into()
    } else {
        normalized
    }
}

fn normalize_request_path(path: &str) -> String {
    if path == "/" {
        return "/".into();
    }

    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        "/".into()
    } else {
        trimmed.into()
    }
}

fn looks_like_asset_path(path: &str) -> bool {
    let last_segment = path.rsplit('/').next().unwrap_or_default();
    last_segment.contains('.')
}

fn content_type_for_path(path: &str) -> &'static str {
    match path.rsplit('.').next() {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("wasm") => "application/wasm",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("ico") => "image/x-icon",
        _ => "application/octet-stream",
    }
}

fn cache_control_for_path(path: &str) -> &'static str {
    if path == "/index.html" {
        "no-cache"
    } else {
        "public, max-age=31536000, immutable"
    }
}

#[cfg(test)]
mod tests {
    use super::{looks_like_asset_path, normalize_archive_path, normalize_request_path};

    #[test]
    fn normalize_archive_path_strips_dot_prefix() {
        let path = std::path::Path::new("./assets/app.js");
        assert_eq!(normalize_archive_path(path), "/assets/app.js");
    }

    #[test]
    fn normalize_request_path_trims_trailing_slashes() {
        assert_eq!(
            normalize_request_path("/devices/board-alpha/"),
            "/devices/board-alpha"
        );
        assert_eq!(normalize_request_path("/"), "/");
    }

    #[test]
    fn looks_like_asset_path_checks_extension() {
        assert!(looks_like_asset_path("/assets/app.js"));
        assert!(!looks_like_asset_path("/devices/board-alpha"));
    }
}
