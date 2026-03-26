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
2. Run the server:

```bash
cargo run -p device-hub-server
```

3. Run the frontend in dev mode:

```bash
trunk serve frontend/index.html
```
