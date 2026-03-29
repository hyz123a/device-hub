# Device Hub Architecture

## Goal

Build a local-first device operations platform that unifies SSH access, serial logs, generic J-Link actions, logs, and AI-callable workflows behind a Web UI and local HTTP server.

## Recommended Project Layout

```text
device-hub/
  server/        # Rust axum local HTTP server
  shared/        # Shared Rust schemas and domain types
  adapters/      # SSH, serial, and generic J-Link adapters
  frontend/      # Yew/WASM full frontend app
  docs/          # Architecture and implementation notes
  tools/         # Dev scripts, packaging helpers, local tooling
```

## Component Roles

### `server/`

- Hosts the local HTTP API, WebSocket/SSE streams, and embedded web assets.
- Owns device registry, job lifecycle, locking, audit logs, and event fan-out.
- Executes adapter actions as structured jobs instead of exposing raw terminal access.

Suggested modules:

```text
server/src/
  main.rs
  app.rs
  config.rs
  error.rs
  api/
  services/
  storage/
  events/
```

### `shared/`

- Stores domain types shared by server and WASM UI.
- Keeps request/response schemas aligned.
- Reduces duplicate JSON contract definitions.

Example types:

- `Device`
- `ProbeInfo`
- `Job`
- `JobStatus`
- `LogEvent`
- `ActionRequest`

### `adapters/`

- Encapsulates external tool and protocol integration.
- Keeps transport-specific behavior out of API handlers.

First adapters:

- `ssh/`
  - connect test
  - exec
  - reboot
  - log follow
- `serial/`
  - open
  - log follow
  - close
- `jlink/`
  - probe
  - flash
  - verify
  - reset
  - erase

Adapter rule of thumb:

- expose structured operations
- return structured results
- stream logs/events during execution
- never make the API depend on free-form shell text

### `frontend/`

- Full Yew/WASM frontend — entire UI written in Rust, compiled to WASM.
- No TypeScript or JS framework; shares types directly from `shared/`.
- Uses `yew_router` for client-side routing and `#[function_component]` for UI components.
- Connects to server via `fetch` and WebSocket through `gloo` or `web-sys`.

Suggested structure:

```text
frontend/src/
  main.rs          # WASM entry point
  app.rs           # root component + router
  pages/
    devices.rs
    device_detail.rs
    job_detail.rs
    log_viewer.rs
  components/      # reusable UI components
  api.rs           # HTTP + WS client bindings
```

Recommended first screens:

- device list
- device detail
- job detail
- live log viewer

## Device Configuration

Devices are declared in a YAML file loaded at startup. Changing the file requires a server restart; live reload is intentionally unsupported to avoid inconsistency while jobs are running.

`ssh`, `serial`, and `jlink` are independent, optional capabilities. A device may have any combination depending on its role:

```yaml
# devices.yaml
devices:
  - id: board-01
    label: "Linux Board"        # SSH only — runs an OS
    ssh:
      host: 192.168.1.100
      user: root
      key: ~/.ssh/id_rsa

  - id: board-02
    label: "Bare Metal Target"  # Serial + J-Link — no OS, debug probe attached
    serial:
      port: /dev/ttyUSB0
      baud: 115200
    jlink:
      serial: "123456789"
      device: STM32H743

  - id: board-03
    label: "Full Dev Board"     # SSH + serial + J-Link
    ssh:
      host: 192.168.1.102
      user: root
      key: $SSH_KEY_PATH        # supports $ENV_VAR substitution
    serial:
      port: /dev/ttyUSB1
      baud: 115200
    jlink:
      serial: "987654321"
      device: STM32H743
```

Sensitive fields (passwords, key paths) support `$ENV_VAR` substitution to avoid storing secrets in the file.

## Runtime Model

### Control Plane

- Browser talks only to local HTTP server.
- Server owns all device actions.
- AI skills call the same HTTP API instead of running shell commands directly.

### Jobs

- Every stateful or long-running action becomes a job, including `ssh.tail`.
- API returns a `job_id` immediately.
- Execution progress and logs flow through a unified event stream.
- Jobs are cancelled via `DELETE /jobs/{job_id}`; the server signals the adapter to stop and transitions the job to `cancelled`.

Job states:

- `pending`
- `running`
- `succeeded`
- `failed`
- `cancelled`

### Events and Logs

Unify all logs into one event format:

```json
{
  "ts": "2026-03-26T12:00:00Z",
  "device_id": "board-01",
  "job_id": "job_123",
  "source": "jlink",
  "channel": "stdout",
  "level": "info",
  "message": "Downloading file..."
}
```

`channel` is the transport layer (`stdout`, `stderr`, `system`).
`level` is the semantic severity (`debug`, `info`, `warn`, `error`).

Primary sources:

- `ssh`
- `serial`
- `jlink`
- `system`
- `job`

### Device Locking

- One exclusive hardware action per device at a time.
- Shared read-only log subscriptions can remain concurrent.
- Flash, erase, and reset always acquire an exclusive lock.
- Lock requests on a busy device return **409 immediately** with the holding job ID:
  ```json
  { "error": "device_locked", "held_by_job_id": "job_123" }
  ```
- Locks carry a TTL (default 60 s); the running job renews via heartbeat.
- On job completion (succeed / fail / cancel) the lock is released immediately.
- A background ticker sweeps expired locks as a safety net.

## API Shape

```text
GET    /devices
GET    /devices/{device_id}

POST   /devices/{device_id}/actions/ssh.exec
POST   /devices/{device_id}/actions/ssh.tail
POST   /devices/{device_id}/actions/ssh.reboot
POST   /devices/{device_id}/actions/serial.tail

POST   /devices/{device_id}/actions/jlink.probe
POST   /devices/{device_id}/actions/jlink.flash
POST   /devices/{device_id}/actions/jlink.verify
POST   /devices/{device_id}/actions/jlink.reset
POST   /devices/{device_id}/actions/jlink.erase

GET    /jobs/{job_id}
GET    /jobs/{job_id}/events
DELETE /jobs/{job_id}

WS     /ws
```

`ssh.tail` and `serial.tail` follow the same job lifecycle as any other action; use `DELETE /jobs/{job_id}` to stop them.

### WebSocket `/ws`

Subscribe to event streams via query parameters:

```text
WS /ws                            # all events
WS /ws?device_id=board-01         # events for one device
WS /ws?job_id=job_123             # events for one job
```

- Server pushes `LogEvent` JSON frames.
- On reconnect, pass `?since=<iso8601_ts>` to replay buffered events (server retains the most recent ~1000 events per device).

## Build Strategy Now

Keep the first version simple:

- use `cargo` + `trunk` for Yew/WASM builds (`trunk serve` for dev, `trunk build` for release)
- `trunk` outputs static assets to `frontend/dist/`; `server/` embeds them via `include_dir!` or serves them from disk in dev
- avoid Bazel until the product shape is stable

## Smooth Migration Path to Bazel Later

When the project grows, adopt the same ideas as the reference repo without copying all of its complexity at once.

Recommended migration order:

1. keep Rust crates and `frontend/` cleanly separated now
2. introduce `shared/` so contracts are reusable across server and WASM
3. standardize output artifacts for server and WASM bundles
4. move to Bazel only when multi-target reproducible builds become important

What Bazel would help with later:

- unified Rust + JS/WASM builds
- reproducible CI builds
- cross-target outputs for `linux/amd64` and `linux/arm64`
- embedded asset packaging
- container image generation

## Why This Fits the Project

- `server/` keeps hardware and tool access local and controlled.
- `shared/` makes server and UI speak the same typed protocol with zero duplication.
- `adapters/` isolates SSH, serial, and J-Link complexity.
- `frontend/` keeps the entire stack in Rust — no TypeScript/JSON schema drift.

## Near-Term Implementation Order

1. create `shared/` + `server/` skeleton together (`axum`, config loader, error types)
2. add job manager and event bus to `server/` (types come from `shared/`)
3. implement `adapters/ssh`
4. implement `adapters/serial`
5. implement `adapters/jlink` on top of `JLinkExe`
6. scaffold `frontend/` with `trunk` + `yew` + `yew_router`, device list page
7. build out remaining pages and live log viewer
8. embed `frontend/dist/` into the server binary for single-binary distribution
