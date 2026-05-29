---
name: device-hub-user
description: Use the local Device Hub service as an API-first operator, including inspect devices, run supported actions, review job evidence, and inspect filtered live or historical logs.
license: MIT
compatibility: opencode
metadata:
  audience: users
  service: device-hub
  workflow: service-operator
---

## What I do

- Help users operate the local Device Hub service primarily through its HTTP API.
- Check whether the backend is running before attempting device actions.
- Guide or perform common flows: list devices, inspect one device, start an action, watch job status, inspect live or historical logs, clear the in-memory log cache, review evidence, and rerun a job.
- Keep responses grounded in the actual service responses and supported actions.

## When to use me

- The user wants to use Device Hub as an operator rather than modify its code.
- The user asks to run or inspect device actions like `ssh.exec`, `ssh.tail`, `ssh.reboot`, `serial.tail`, `serial.write`, or `serial.exec`.
- The user wants to look at job evidence, filtered logs, recent device logs without a job id, live streams, timeout behavior, or rerun a previous job.
- The user wants to clear or flush log cache without deleting persisted job evidence or artifacts.
- The user reports that the local API is unavailable and wants help checking service health.

## Service endpoints

- Backend base URL: `http://127.0.0.1:3000`

Adjust this only if the running environment or user explicitly says it differs.

## Supported user flows

1. Health check

```bash
curl -s http://127.0.0.1:3000/api/devices
```

2. List devices

```bash
curl -s http://127.0.0.1:3000/api/devices
```

3. Inspect one device

```bash
curl -s http://127.0.0.1:3000/api/devices/<device-id>
```

Notes:

- Device detail includes `runtime.active_tails`.
- An active serial tail appears with `action`, `job_id`, and `active_since`.
- Check this before scanning `/api/jobs` when you only need to find the current live tail for a device.

4. Start an action

Examples:

```bash
curl -s -X POST http://127.0.0.1:3000/api/devices/<device-id>/actions/ssh.exec \
  -H 'content-type: application/json' \
  -d '{"payload":"uname -a"}'

curl -s -X POST http://127.0.0.1:3000/api/devices/<device-id>/actions/ssh.tail \
  -H 'content-type: application/json' \
  -d '{"payload":"/var/log/syslog"}'

curl -s -X POST http://127.0.0.1:3000/api/devices/<device-id>/actions/ssh.tail

curl -s -X POST http://127.0.0.1:3000/api/devices/<device-id>/actions/serial.tail \
  -H 'content-type: application/json' \
  -d 'null'

curl -s -X POST 'http://127.0.0.1:3000/api/devices/<device-id>/actions/serial.tail?reuse=true' \
  -H 'content-type: application/json' \
  -d 'null'

curl -s -X POST http://127.0.0.1:3000/api/devices/<device-id>/actions/serial.write \
  -H 'content-type: application/json' \
  -d '{"payload":"help\n"}'

curl -s -X POST http://127.0.0.1:3000/api/devices/<device-id>/actions/serial.exec \
  -H 'content-type: application/json' \
  -d '{"payload":"ip route"}'

curl -s -X POST http://127.0.0.1:3000/api/devices/<device-id>/actions/serial.exec \
  -H 'content-type: application/json' \
  -d '{"payload":"{\"command\":\"iwpriv ra0 show psinfo\",\"timeout_secs\":5,\"until\":\"# \"}"}'
```

5. Execute a serial command and return captured output directly

Use this when the user wants command output without manually inspecting a tail stream. If an active `serial.tail` exists for the device, Device Hub reuses that tail session for command/expect capture; otherwise it opens a temporary serial session for the command.

```bash
curl -s -X POST http://127.0.0.1:3000/api/devices/<device-id>/serial/exec \
  -H 'content-type: application/json' \
  -d '{"command":"ip route","timeout_secs":5,"until":"# "}'
```

Notes:

- Action start responses include the normal job fields plus `already_running`.
- For ordinary starts, `already_running` is `false`.
- For `serial.tail?reuse=true`, Device Hub returns the active tail job and sets `already_running=true` instead of opening the serial port again.
- Without `reuse=true`, starting `serial.tail` keeps the normal contract: it attempts to create a new job.
- `serial.exec` is available as either an asynchronous job action or a synchronous `/serial/exec` API.
- The synchronous `/serial/exec` response includes `command`, `output`, `matched`, and `timed_out`.
- The synchronous `/serial/exec` API can run while `serial.tail` is active; it reuses the active tail session instead of requiring the tail to be stopped.
- Prefer synchronous `/serial/exec` for one-shot commands such as `ip route`, `iwpriv ra0 show psinfo`, or `cat /proc/net/nf_conntrack`.
- Prefer asynchronous `actions/serial.exec` when durable job evidence and downloadable `output.txt` / `result.json` artifacts are needed.

6. Check job status and events

```bash
curl -s http://127.0.0.1:3000/api/jobs/<job-id>
curl -s http://127.0.0.1:3000/api/jobs/<job-id>/events
curl -s http://127.0.0.1:3000/api/jobs/<job-id>/evidence
```

7. Watch live events over WebSocket

Examples:

```text
ws://127.0.0.1:3000/api/ws?device_id=<device-id>
ws://127.0.0.1:3000/api/ws?device_id=<device-id>&source=serial
ws://127.0.0.1:3000/api/ws?device_id=<device-id>&source=ssh&search=timeout
ws://127.0.0.1:3000/api/ws?device_id=<device-id>&structured_only=true
```

Supported WebSocket filters:

- `device_id=<device-id>`
- `job_id=<job-id>`
- `since=<RFC3339 timestamp>`
- `source=ssh|serial|jlink|system|job`
- `level=debug|info|warn|error`
- `search=<case-insensitive substring match against message, device_id, job_id, source, channel, level>`
- `structured_only=true|false`

Notes:

- The stream sends raw `LogEvent` JSON objects.
- `serial.tail` preserves terminal control bytes such as carriage returns so terminal-style clients can render the stream faithfully.

8. Filter job events for debugging

```bash
curl -s "http://127.0.0.1:3000/api/jobs/<job-id>/events?source=serial"
curl -s "http://127.0.0.1:3000/api/jobs/<job-id>/events?level=error"
curl -s "http://127.0.0.1:3000/api/jobs/<job-id>/events?source=serial&search=panic"
curl -s "http://127.0.0.1:3000/api/jobs/<job-id>/events?source=serial&level=error&search=timeout"
```

Supported filters:

- `source=ssh|serial|jlink|system|job`
- `level=debug|info|warn|error`
- `search=<case-insensitive substring match against message, device_id, job_id, source, channel, level>`

9. Query recent cached events without a job id

```bash
curl -s "http://127.0.0.1:3000/api/events?device_id=<device-id>&source=serial&since=5m"
curl -s "http://127.0.0.1:3000/api/events?device_id=homebase1,camera1&search=history&since=5m"
curl -s "http://127.0.0.1:3000/api/events?device_id=<device-id>&search_any=reset,key,factory_reset,button_reset,KEY_UNBIND&since=5m"
curl -s "http://127.0.0.1:3000/api/events?device_id=<device-id>&search_regex=wifi_link%7CKEEP_NORMAL&since=5m"
```

Supported filters:

- `device_id=<device-id>` or comma-separated ids such as `device_id=homebase1,camera1`
- `source=ssh|serial|jlink|system|job`
- `level=debug|info|warn|error`
- `search=<case-insensitive substring match against message, device_id, job_id, source, channel, level>`
- `search_any=<comma-separated case-insensitive OR terms>`
- `search_regex=<Rust regex matched against event text>`
- `since=<RFC3339 timestamp | relative window like 30s, 5m, or 2h>`

Notes:

- `GET /api/events` reads only the current in-memory event cache.
- It is useful when the operator knows the device and recent time window but not the job id.
- Regex uses Rust `regex` syntax, not Elasticsearch or Lucene query DSL.
- Invalid regex patterns return a bad request instead of deleting or mutating logs.

10. Download artifacts when needed

```bash
curl -OJ http://127.0.0.1:3000/api/jobs/<job-id>/artifacts/job.json
curl -OJ http://127.0.0.1:3000/api/jobs/<job-id>/artifacts/events.ndjson
curl -OJ http://127.0.0.1:3000/api/jobs/<job-id>/artifacts/output.txt
curl -OJ http://127.0.0.1:3000/api/jobs/<job-id>/artifacts/result.json
```

11. Clear the in-memory log cache

```bash
curl -s -X DELETE http://127.0.0.1:3000/api/events
```

Notes:

- This flushes only the server's in-memory event cache used for live replay.
- It does not delete persisted job evidence, job metadata, `events.ndjson`, or adapter artifacts.
- Use `/api/jobs/<job-id>/evidence` or artifact downloads when historical evidence must remain inspectable after the flush.

12. Rerun a job with the same inputs

```bash
curl -s -X POST http://127.0.0.1:3000/api/jobs/<job-id>/rerun
```

13. Cancel a running job

```bash
curl -s -X DELETE http://127.0.0.1:3000/api/jobs/<job-id>
```

## Workflow

1. Confirm whether the user wants API execution, log inspection, or service troubleshooting.
2. Check backend availability before running actions.
3. If the task targets a device action, list devices first and verify the target device supports the requested action.
4. For actions that require payloads, confirm or infer the exact payload:

- `ssh.exec` requires a command string.
- `ssh.tail` uses the provided log path, or falls back to the device `ssh.default_log_path` when omitted.
- `serial.tail` does not require a payload.
- `serial.write` requires a payload string.
- `serial.write` payloads may be raw text, newline-terminated commands, or control characters like Ctrl-C.
- `serial.exec` requires a command. The synchronous API accepts structured JSON: `command`, optional `timeout_secs`, and optional `until`.
- The asynchronous `actions/serial.exec` endpoint accepts a plain command string in `payload`, or a JSON-encoded `SerialExecRequest` string when timeout or marker matching is needed.
- `ssh.reboot` does not require a payload.
- `serial.tail?reuse=true` reuses an active tail and returns `already_running=true`; use it when the user explicitly wants the existing stream.

5. Start the action and capture the returned `job_id`.
6. If the user wants to run a serial command and receive the command output, prefer this order:

- Use `POST /api/devices/<device-id>/serial/exec` with `command`, a bounded `timeout_secs`, and an `until` prompt marker when known.
- Do not stop an active `serial.tail` first; `/serial/exec` reuses it automatically and falls back to a temporary serial session when no tail is active.
- Report `output`, `matched`, and `timed_out` from the response.
- If `timed_out=true`, include the partial captured output and suggest adjusting `until` or timeout only when useful.

7. If the user wants an auditable serial command run, start `actions/serial.exec` and then inspect job evidence or download `output.txt` / `result.json`.
8. If the user wants to send serial input during a live interactive session, prefer this order:

- Inspect `GET /api/devices/<device-id>` and check `runtime.active_tails` for an active `serial.tail`.
- If no tail is active, start `serial.tail`; if a tail is active, reuse that `job_id`.
- Start `serial.write` with the intended payload.
- Inspect the tail job events for device output and the write job for write confirmation.

9. For follow-up inspection, prefer this order:

- `/api/jobs/<job-id>` for status
- `/api/ws?...` for live filtered streaming when the user is actively watching output
- `/api/events?...` for recent cached logs when the job id is unknown
- `/api/jobs/<job-id>/events?source=...&level=...&search=...` for targeted debugging
- `/api/jobs/<job-id>/evidence` for summary and artifacts
- `/api/jobs/<job-id>/events` for full history

10. If the user asks to clear or flush logs, distinguish between memory cache and persisted evidence:

- Use `DELETE /api/events` to flush the in-memory event cache.
- Do not imply this deletes `/api/jobs/<job-id>/evidence` or artifact files.
- After flushing, reconnecting to `/api/ws` will not replay the cleared in-memory events, but persisted job evidence remains available.

11. If the user wants another attempt, use the rerun endpoint instead of reconstructing arguments manually when possible.
12. Prefer filtered event queries over downloading artifacts when the goal is quick debugging.
13. Report outcomes in operator language: device, action, job ID, current status, key evidence, and any next step.

## What not to include

- Do not explain the frontend stack, UI affordances, Rust crates, or repo structure unless the user explicitly asks.
- Do not mention internal config file locations or implementation modules when the task is simply to use the service.
- Do not describe internal job runner details unless the user asks for timeout or execution behavior.

## Response pattern

Use this compact structure when acting as an operator:

```text
Device: <device-id>
Action: <action>
Job: <job-id or n/a>
Status: <status>
Evidence: <short summary or n/a>
Next: <useful next step or none>
```

## Guardrails

- Do not claim a device supports an action unless the API/config confirms it.
- Do not invent payloads for `ssh.exec`, `serial.write`, or `serial.exec`; ask one short question if the value is missing.
- For `ssh.tail`, ask only when the payload is missing and the device does not have `ssh.default_log_path` configured.
- Prefer using the rerun endpoint for repeat attempts so the same stored inputs are reused.
- If the backend is down, say so clearly and help restart or diagnose it before issuing device commands.
- Treat long-running tails as ongoing jobs; cancel them explicitly instead of assuming they stop on their own.
- Treat `serial.write` as a short-lived job that confirms bytes were sent, not as the place to inspect device output.
- Treat synchronous `/serial/exec` as the preferred path for one-shot command/expect output, including when a `serial.tail` is already active.
- Treat asynchronous `actions/serial.exec` as the preferred path when the user needs durable job evidence.
- Treat log cache flushing as an in-memory replay cleanup only; never describe it as evidence deletion.
- Treat `GET /api/events` as a cache query, not persisted evidence; use job evidence or artifacts for durable history.
- For regex event searches, use Rust `regex` syntax and URL-encode special characters such as `|`.
- For AI-driven debugging, prefer concise API queries, filtered WebSocket streams, and filtered event endpoints over UI walkthroughs.

## Customization notes

Users can copy this skill folder and adjust only operator-facing details:

- Base URL if the service does not run on `3000`
- Supported action examples if the deployment exposes more or fewer actions
- Response format if they prefer more concise or more verbose operator output
