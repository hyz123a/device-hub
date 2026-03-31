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
- [ ] Add artifact export/download flow for completed jobs
- [x] Add `serial` support to the shared schema, device detail page, and server action routing so `serial.tail` becomes a first-class job like `ssh.tail`
- [x] Implement a `device-hub-adapter-serial` crate with bounded log streaming, cancellation support, and unified `LogEvent` emission
- [x] Add `serial.write` job support and a basic UI flow so operators can send serial input, control characters, and newline-terminated commands through Device Hub
- [x] Add J-Link action endpoints and UI controls for `jlink.probe`, `jlink.flash`, `jlink.verify`, and `jlink.reset`
- [x] Extend the J-Link adapter to return structured result payloads and artifact files instead of relying only on raw tool text
- [x] Add per-action timeout and safe teardown policies in the job runner so failed or cancelled jobs leave devices in a known state
- [x] Surface richer job evidence in the Web UI, including artifact lists, download links, and action-specific summaries for AI/human diagnosis
- [x] Add an API/UI flow to rerun a previous job with the same device, action, and arguments to tighten the harness-debug loop
- [ ] Add server and integration tests covering serial streaming, J-Link job lifecycle, timeout handling, and artifact persistence
- [x] Embed `frontend/dist` into `server` for single-binary local distribution
