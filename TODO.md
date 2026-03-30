# TODO

## Done

- [x] Migrate server HTTP and websocket endpoints to `/api/...` to avoid frontend route conflicts
- [x] Update frontend API and websocket calls to use `/api` paths
- [x] Update `frontend/Trunk.toml` and docs for `/api` routing
- [x] Run server and frontend verification after route migration

## Next

- [x] Add action controls to `frontend/src/pages/device_detail.rs` so the Web UI can start `ssh.exec`, `ssh.tail`, and `ssh.reboot`
- [x] Add a create-job API helper in `frontend/src/api.rs` and redirect to `/jobs/:job_id` after starting an action
- [x] Show stdout, stderr, and system events more clearly in the job and live-log views
- [x] Implement device locking in the server so conflicting actions return `409` with the holding job ID
- [x] Add API integration tests for device lock conflicts and job cancellation semantics
- [ ] Persist or export job output artifacts so completed runs remain inspectable outside the live event buffer
