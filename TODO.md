# TODO

## Done

- [x] Migrate server HTTP and websocket endpoints to `/api/...` to avoid frontend route conflicts
- [x] Update frontend API and websocket calls to use `/api` paths
- [x] Update `frontend/Trunk.toml` and docs for `/api` routing
- [x] Run server and frontend verification after route migration
- [x] Add action controls to `frontend/src/pages/device_detail.rs` so the Web UI can start `ssh.exec`, `ssh.tail`, and `ssh.reboot`
- [x] Add a create-job API helper in `frontend/src/api.rs` and redirect to `/jobs/:job_id` after starting an action
- [x] Show stdout, stderr, and system events more clearly in the job and live-log views
- [x] Implement device locking in the server so conflicting actions return `409` with the holding job ID
- [x] Add API integration tests for device lock conflicts and job cancellation semantics
- [x] Persist job output artifacts as `job.json` + `events.ndjson` so completed runs remain inspectable outside the live event buffer
- [x] Migrate `frontend` styling to Tailwind CSS by scanning `frontend/src/**/*.rs`, compiling a generated stylesheet, and switching `frontend/index.html` to load it

## Next
- [x] Add `serial.exec` command/expect support with shared execution core, job evidence, and synchronous API response
- [x] Add global event query API with richer filters
  - Support `GET /api/events` against the current server event cache.
  - Filters should include `device_id`, `source`, `level`, `search`, `search_any`, `search_regex`, and `since`.
  - `device_id` should support multiple devices, e.g. `device_id=homebase1,camera1`.
  - `search_any` should support multi-keyword OR matching, e.g. `reset,key,factory_reset,button_reset,KEY_UNBIND`.
  - `since` should accept both RFC3339 timestamps and relative windows such as `5m`, `30s`, and `2h`.
  - Regex search should use Rust `regex` semantics over event fields, not Elasticsearch/Lucene query DSL semantics.
- [x] Add active tail discovery and explicit tail reuse mode
  - Device detail should expose the currently active serial tail, including `job_id` and `active_since`.
  - Prefer a runtime/status structure that can later also represent active SSH tails or device lock ownership.
  - This should remove the need to manually scan `/api/jobs` after errors such as `port already in use by an active serial.tail job`.
  - `serial.tail` should support an explicit reuse mode so callers can reuse the active tail instead of failing when the port is already open.
  - Prefer an opt-in request shape such as `reuse=true` to preserve the normal "start action creates a new job" contract.
  - Response should make reuse explicit, e.g. `already_running=true` with the reused `job_id`.
- [x] Add an implfuture-style Vite bundle for `xterm.js` and switch the log panel to a terminal-like visual renderer focused on presentation quality
- [x] Refactor `server` toward a real hexagonal architecture by introducing explicit ports/use cases and removing direct core dependencies on concrete adapters
- [x] Add artifact export/download flow for completed jobs
- [x] Add `serial` support to the shared schema, device detail page, and server action routing so `serial.tail` becomes a first-class job like `ssh.tail`
- [x] Implement a `device-hub-adapter-serial` crate with bounded log streaming, cancellation support, and unified `LogEvent` emission
- [x] Add `serial.write` job support and a basic UI flow so operators can send serial input, control characters, and newline-terminated commands through Device Hub
- [x] Add J-Link action endpoints and UI controls for `jlink.probe`, `jlink.flash`, `jlink.verify`, and `jlink.reset`
- [x] Extend the J-Link adapter to return structured result payloads and artifact files instead of relying only on raw tool text
- [x] Add per-action timeout and safe teardown policies in the job runner so failed or cancelled jobs leave devices in a known state
- [x] Surface richer job evidence in the Web UI, including artifact lists, download links, and action-specific summaries for AI/human diagnosis
- [x] Add an API/UI flow to rerun a previous job with the same device, action, and arguments to tighten the harness-debug loop
- [x] Add server and integration tests covering serial streaming, J-Link job lifecycle, timeout handling, and artifact persistence
- [x] Embed `frontend/dist` into `server` for single-binary local distribution
- [x] Add a persistent device registry port and storage backend so devices can be managed without editing config files
- [x] Add server API endpoints and validation for creating, updating, and deleting devices with SSH, serial, and J-Link settings
- [x] Add tests covering device CRUD, validation, and registry persistence across server restarts
- [x] Add frontend device management flows for creating, editing, and removing devices
- [x] Add adapter support and stable server seams for serial port enumeration and J-Link probe scanning                                   
- [x] Add device page selectors that let operators choose a scanned serial path or J-Link probe while still allowing manual entry           
- [x] Add tests covering discovery API responses and device-page scan-driven selection flows
- [x] Profile the frontend live-log viewer to identify the main rendering and popup-open bottlenecks with large log volumes
- [x] Optimize append rendering and scroll behavior for long-running logs so streaming remains responsive under load
- [x] Freeze viewport anchoring while the user has an active text selection so copied content does not drift as new logs arrive
- [x] Add verification for large static logs, sustained streaming logs, and text-selection copy behavior in the frontend log viewer
