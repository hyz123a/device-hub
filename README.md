# Device Hub

Local-first device control plane for AI-assisted hardware workflows.

Device Hub turns common hardware operations into structured jobs with APIs, evidence, logs, timeouts, and a simple Web UI.

## Current Capabilities

- Device registry loaded from `devices.yaml`
- SSH actions:
  - `ssh.exec`
  - `ssh.tail`
  - `ssh.reboot`
- Serial actions:
  - `serial.tail`
  - `serial.write`
- J-Link actions:
  - `jlink.probe`
  - `jlink.flash`
  - `jlink.verify`
  - `jlink.reset`
- Unified job model with:
  - live events over WebSocket
  - persisted job metadata and event history
  - artifact download endpoints
  - rerun flow for previous jobs
  - device locking for conflicting actions
  - per-action timeout and teardown policy reporting
- Yew frontend for devices, jobs, evidence, logs, and serial interaction
- Single-binary local distribution by embedding `frontend/dist` into the server

## Workspace

- `shared/`: shared domain types for actions, jobs, devices, and events
- `server/`: axum HTTP server, websocket stream, artifact storage, and job runtime
- `adapters/ssh/`: SSH action execution
- `adapters/serial/`: serial tailing and serial write support
- `adapters/jlink/`: J-Link probe/flash/verify/reset support and structured artifacts
- `frontend/`: Yew/WASM frontend with Tailwind CSS
- `docs/`: product notes, plans, and workflow documentation

## Quick Start

1. Copy `devices.example.yaml` to `devices.yaml` and fill in real values.

```bash
cp devices.example.yaml devices.yaml
```

2. Run the server API.

```bash
cargo run -p device-hub-server
```

The server listens on `http://127.0.0.1:3000` and serves both `/api/...` and the embedded frontend when a bundle is available.

3. Run the frontend in dev mode from `frontend/`.

```bash
cd frontend
npm install
trunk serve --config Trunk.toml
```

Notes:

- `Trunk.toml` runs `npm run build:css` as a pre-build hook.
- The dev frontend listens on `http://127.0.0.1:8089`.
- `/api/devices`, `/api/jobs`, and `/api/ws` are proxied to the local server.

## Device Config

Example `devices.yaml` entries can include SSH, serial, and J-Link settings:

```yaml
devices:
  - id: board-01
    label: Linux Board
    ssh:
      host: 192.168.110.151
      user: root
      key: /home/hyz/.ssh/id_rsa
      default_log_path: /tmp/sdcard/debug_log
    serial:
      port: /dev/ttyUSB0
      baud: 115200

  - id: board-02
    label: Bare Metal Target
    jlink:
      serial: "123456789"
      device: STM32H743
      interface: SWD
      speed_khz: 4000
      autoconnect: true
```

Capabilities are inferred from the config:

- devices with `ssh` support SSH actions
- devices with `serial` support serial tail and write
- devices with `jlink` support J-Link actions

## Common Flows

### Start an action

```bash
curl -s -X POST http://127.0.0.1:3000/api/devices/<device-id>/actions/ssh.exec \
  -H 'content-type: application/json' \
  -d '{"payload":"uname -a"}'
```

```bash
curl -s -X POST http://127.0.0.1:3000/api/devices/<device-id>/actions/ssh.tail
```

When the device has `ssh.default_log_path` configured, `ssh.tail` can omit the payload and uses that default path. Supplying a payload still overrides the device default for that job.

```bash
curl -s -X POST http://127.0.0.1:3000/api/devices/<device-id>/actions/serial.write \
  -H 'content-type: application/json' \
  -d '{"payload":"help\n"}'
```

```bash
curl -s -X POST http://127.0.0.1:3000/api/devices/<device-id>/actions/jlink.probe \
  -H 'content-type: application/json' \
  -d 'null'
```

### Inspect a job

```bash
curl -s http://127.0.0.1:3000/api/jobs/<job-id>
curl -s http://127.0.0.1:3000/api/jobs/<job-id>/evidence
curl -s http://127.0.0.1:3000/api/jobs/<job-id>/events
```

### Filter job events

```bash
curl -s "http://127.0.0.1:3000/api/jobs/<job-id>/events?source=serial"
curl -s "http://127.0.0.1:3000/api/jobs/<job-id>/events?level=error"
curl -s "http://127.0.0.1:3000/api/jobs/<job-id>/events?source=serial&search=panic"
```

### Download artifacts

```bash
curl -OJ http://127.0.0.1:3000/api/jobs/<job-id>/artifacts/job.json
curl -OJ http://127.0.0.1:3000/api/jobs/<job-id>/artifacts/events.ndjson
```

### Rerun a previous job

```bash
curl -s -X POST http://127.0.0.1:3000/api/jobs/<job-id>/rerun
```

### Watch live events

Use the Web UI or connect to:

```text
ws://127.0.0.1:3000/api/ws
```

## Serial Interaction

The intended serial operator flow is:

1. start `serial.tail`
2. send `serial.write`
3. inspect the tail job for device output
4. inspect the write job for confirmation and evidence

`serial.write` supports:

- raw text
- newline-terminated commands
- control characters such as Ctrl-C, Ctrl-D, and Escape

## Job Evidence

Each job can produce:

- job status and timestamps
- structured summary data
- event history
- artifacts such as `job.json`, `events.ndjson`, and adapter-specific files

This makes completed runs inspectable even after the live event buffer rolls over.

## Single-Binary Build

Build the frontend bundle and embed it into the server binary:

```bash
bash tools/build-server-with-frontend.sh
```

That script:

- installs frontend npm dependencies on first run
- builds `frontend/dist`
- packs it into a tar bundle
- passes the bundle path into the server build through `DEVICE_HUB_FRONTEND_BUNDLE`

The first release frontend build may take longer because `trunk` can download `wasm-opt`.

After that bundle exists at `target/frontend-bundle/frontend.bundle.tar`, ordinary server builds can reuse it:

```bash
cargo build -p device-hub-server
```

When no frontend bundle exists yet, the server falls back to a placeholder frontend.

## API Overview

Main API routes:

- `GET /api/`
- `GET /api/devices`
- `GET /api/devices/{device_id}`
- `POST /api/devices/{device_id}/actions/{action}`
- `GET /api/jobs`
- `GET /api/jobs/{job_id}`
- `DELETE /api/jobs/{job_id}`
- `GET /api/jobs/{job_id}/evidence`
- `GET /api/jobs/{job_id}/events`
- `POST /api/jobs/{job_id}/rerun`
- `GET /api/jobs/{job_id}/artifacts/{artifact_name}`
- `GET /api/ws`

## Status

This project is no longer just a scaffold. It already supports an end-to-end local device workflow with:

- action execution
- unified logs
- persisted evidence
- rerun
- frontend interaction
- embedded local distribution

The remaining work is mostly about expanding coverage, tightening tests, and adding more hardware-facing adapters and workflows.
