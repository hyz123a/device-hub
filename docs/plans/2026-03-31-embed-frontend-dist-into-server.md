# Embed Frontend Dist Into Server

## Summary

This change packages the built Yew frontend as a single bundle artifact and embeds that bundle into the Rust server so local distribution can ship one binary while leaving room for a future Bazel build.

## Problem

- The current server only exposes `/api/...` routes and does not serve the built frontend directly.
- Local use still depends on a separate frontend build output such as `frontend/dist`, which breaks the single-binary distribution goal in `TODO.md`.
- A simple runtime `ServeDir` approach is useful for development, but it still depends on files existing next to the binary.
- Directly embedding a generated directory can work under Cargo, but it is a less stable fit for Bazel because generated outputs are easier to pass around as declared files than as ad hoc source-tree directories.

## Goals

- Ship one local server binary that can serve the Web UI without an external `frontend/dist` directory at runtime.
- Keep `/api/...` behavior unchanged.
- Support SPA fallback so browser refreshes on `/devices/:id` and `/jobs/:id` still load the frontend.
- Make the embedding input build-system friendly so Cargo today and Bazel later can both provide it.

## Non-Goals

- Replace local frontend development with an embedded-asset workflow.
- Introduce SSR or server-side rendering.
- Commit generated frontend build output into the repository.

## Recommended Design

Use a single frontend bundle file as the contract between the frontend build and the server build:

1. Build `frontend/dist` using the existing Trunk flow.
2. Pack the resulting files into one archive artifact such as `frontend.bundle.tar`.
3. Pass the absolute bundle path into the server build through an environment variable.
4. Embed that bundle into the server with `include_bytes!`.
5. On server startup, expand the archive into an in-memory asset map keyed by request path.
6. Route non-API asset requests to that map and fall back to `index.html` for frontend routes.

This keeps the server-side embedding code independent from how the bundle was produced.

## Why This Over Direct Directory Embedding

- Cargo can support both approaches, but a single bundle file is simpler to feed into `include_bytes!`.
- Bazel works best when a target consumes explicit file outputs from another target.
- The server can treat the frontend as one declared input instead of depending on a build-tree directory layout.
- The packaging boundary is clear: frontend build produces one artifact, server build embeds one artifact.

## Server Changes

### 1. Add a dedicated frontend asset module

Create `server/src/frontend.rs` to own:

- embedded bundle loading
- bundle archive parsing
- MIME type selection
- cache policy metadata
- path normalization for asset lookup
- SPA fallback to `index.html`

Suggested shape:

```rust
pub struct FrontendAsset {
    pub bytes: bytes::Bytes,
    pub content_type: mime::Mime,
    pub cache_control: &'static str,
}

#[derive(Clone)]
pub struct FrontendAssets {
    inner: std::sync::Arc<std::collections::HashMap<String, FrontendAsset>>,
}

impl FrontendAssets {
    pub fn load_from_embedded_bundle() -> Result<Self, FrontendAssetError>;
    pub fn get(&self, path: &str) -> Option<&FrontendAsset>;
    pub fn index(&self) -> Option<&FrontendAsset>;
}
```

The implementation should treat archive entries as web paths such as:

- `/index.html`
- `/assets/app-abc123.js`
- `/assets/app-abc123_bg.wasm`
- `/style.css`

### 2. Extend application state

Add `frontend: FrontendAssets` to `server/src/app.rs` state so handlers can reuse one parsed asset set.

### 3. Add frontend routes outside `/api`

Keep the existing `/api` router unchanged. Add routes for frontend delivery at the top level:

- exact asset requests return the embedded file
- unknown non-API paths return embedded `index.html`
- missing static-like asset paths return `404`

Practical rule:

- requests under `/api` never hit the frontend handler
- requests with a file extension try exact asset lookup first
- extensionless paths fall back to `index.html`

That preserves SPA refresh behavior while avoiding accidental HTML responses for broken asset URLs.

### 4. Return correct response headers

The handler should set at least:

- `Content-Type`
- `Cache-Control`

Suggested cache split:

- `index.html`: `no-cache`
- hashed JS/CSS/WASM/image assets: `public, max-age=31536000, immutable`

## Build Contract

Introduce one environment variable consumed by the server build:

- `DEVICE_HUB_FRONTEND_BUNDLE`

The server build should fail clearly if this variable is missing for release-style embedded builds.

Two workable modes:

1. Strict embed mode
   The build requires `DEVICE_HUB_FRONTEND_BUNDLE` and fails when unset.

2. Dev fallback mode
   Local `cargo test` and server-only work can fall back to a tiny built-in placeholder page or a checked-in minimal asset set.

For this repository, strict embed mode is cleaner if the build/test commands are updated together.

## Cargo Path

Recommended near-term flow:

1. Run frontend CSS build.
2. Run `trunk build --release --dist frontend/dist`.
3. Pack `frontend/dist` into `target/frontend.bundle.tar`.
4. Invoke server build with `DEVICE_HUB_FRONTEND_BUNDLE=/abs/path/to/target/frontend.bundle.tar`.

This can be wrapped in one script under `tools/`, for example:

- `tools/build-frontend-bundle.sh`
- `tools/build-server-with-frontend.sh`

If desired later, a `build.rs` can automate parts of this, but keeping frontend build orchestration outside `build.rs` is easier to debug and easier to mirror in Bazel.

## Bazel Compatibility

The design intentionally avoids coupling server code to `frontend/dist` as a directory.

Later, Bazel can model this as:

1. a frontend target that builds the WASM app and static files
2. a packaging target that emits `frontend.bundle.tar`
3. a `rust_binary` target for `server` that receives that tar as an input and exposes its path via `rustc_env`

Conceptually:

```starlark
rust_binary(
    name = "server",
    srcs = glob(["src/**/*.rs"]),
    compile_data = ["//frontend:bundle"],
    rustc_env = {
        "DEVICE_HUB_FRONTEND_BUNDLE": "$(rootpath //frontend:bundle)",
    },
    # ...
)
```

The exact rule shape may differ, but the important part is unchanged: the server consumes one declared bundle file.

## Reference To `implfuture.dev`

`implfuture.dev` is a useful reference for route shape, not for final asset packaging:

- Good reference:
  - serve built frontend assets from the server process
  - keep API and frontend routing separate
  - return the frontend entry page for recognized client routes
- Not the part to copy directly:
  - `ServeDir::new("bundle/dist")`
  - `ServeDir::new("app_wasm")`

Those still require runtime files on disk, while this task wants single-binary local distribution.

## Testing

Add server/integration coverage for:

- `GET /` returns embedded `index.html`
- `GET /devices/test-device` returns embedded `index.html`
- `GET /jobs/job_123` returns embedded `index.html`
- `GET /api` still returns service metadata JSON
- `GET` for an embedded asset path returns bytes with the expected content type
- `GET` for a missing hashed asset such as `/assets/missing.js` returns `404`

If the bundle loader is split into pure logic plus HTTP handlers, unit-test the archive parsing and path lookup separately from the router tests.

## Rollout Plan

1. Add `server/src/frontend.rs` and wire embedded asset loading into app startup.
2. Add top-level frontend routes with SPA fallback.
3. Add a small bundle-building script for the Cargo workflow.
4. Add tests for root, SPA fallback, API preservation, and asset delivery.
5. Update local build/run documentation to use the embedded frontend flow.

## Why

Using a single embedded bundle gives the project the single-binary local distribution promised in the TODO while keeping the build boundary clean enough to adopt Bazel later without redesigning the server's asset-loading model.
