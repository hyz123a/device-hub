# Device Hub

Local-first device control plane scaffold for AI-assisted hardware workflows.

## Workspace

- `shared/`: shared domain types
- `server/`: axum HTTP server and in-memory runtime services
- `adapters/ssh/`: SSH adapter contract scaffold
- `adapters/jlink/`: J-Link adapter contract scaffold
- `frontend/`: Yew/WASM frontend scaffold

## Quick Start

1. Copy `devices.example.yaml` to `devices.yaml` and fill in real values.
2. Run the server API:

```bash
cargo run -p device-hub-server
```

3. Run the frontend in dev mode from the `frontend/` directory so Trunk picks up `frontend/Trunk.toml` and proxies `/api/devices`, `/api/jobs`, and `/api/ws` to the local server:

```bash
cd frontend
trunk serve --config Trunk.toml
```

## Single-Binary Build

Build the frontend bundle and embed it into the server binary:

```bash
bash tools/build-server-with-frontend.sh
```

That script installs frontend npm dependencies on first run, builds `frontend/dist`, packs it as a single tar bundle, and passes the bundle path into the server build through `DEVICE_HUB_FRONTEND_BUNDLE`.

The first release frontend build may take longer because `trunk` can download `wasm-opt`.

After that bundle exists at `target/frontend-bundle/frontend.bundle.tar`, ordinary `cargo build -p device-hub-server` will reuse it automatically. The server only falls back to the placeholder frontend when no bundle has been built yet.
