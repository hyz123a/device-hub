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
- The user asks to run or inspect device actions like `ssh.exec`, `ssh.tail`, `ssh.reboot`, `serial.tail`, or `serial.write`.
- The user wants to look at job evidence, filtered logs, live streams, timeout behavior, or rerun a previous job.
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

curl -s -X POST http://127.0.0.1:3000/api/devices/<device-id>/actions/serial.write \
  -H 'content-type: application/json' \
  -d '{"payload":"help\n"}'
```

5. Check job status and events

```bash
curl -s http://127.0.0.1:3000/api/jobs/<job-id>
curl -s http://127.0.0.1:3000/api/jobs/<job-id>/events
curl -s http://127.0.0.1:3000/api/jobs/<job-id>/evidence
```

6. Watch live events over WebSocket

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

7. Filter job events for debugging

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

8. Download artifacts when needed

```bash
curl -OJ http://127.0.0.1:3000/api/jobs/<job-id>/artifacts/job.json
curl -OJ http://127.0.0.1:3000/api/jobs/<job-id>/artifacts/events.ndjson
```

9. Clear the in-memory log cache

```bash
curl -s -X DELETE http://127.0.0.1:3000/api/events
```

Notes:

- This flushes only the server's in-memory event cache used for live replay.
- It does not delete persisted job evidence, job metadata, `events.ndjson`, or adapter artifacts.
- Use `/api/jobs/<job-id>/evidence` or artifact downloads when historical evidence must remain inspectable after the flush.

10. Rerun a job with the same inputs

```bash
curl -s -X POST http://127.0.0.1:3000/api/jobs/<job-id>/rerun
```

11. Cancel a running job

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
- `ssh.reboot` does not require a payload.

5. Start the action and capture the returned `job_id`.
6. If the user wants to send serial input and observe the response, prefer this order:

- Start `serial.tail` first.
- Start `serial.write` with the intended payload.
- Inspect the tail job events for device output and the write job for write confirmation.

7. For follow-up inspection, prefer this order:

- `/api/jobs/<job-id>` for status
- `/api/ws?...` for live filtered streaming when the user is actively watching output
- `/api/jobs/<job-id>/events?source=...&level=...&search=...` for targeted debugging
- `/api/jobs/<job-id>/evidence` for summary and artifacts
- `/api/jobs/<job-id>/events` for full history

8. If the user asks to clear or flush logs, distinguish between memory cache and persisted evidence:

- Use `DELETE /api/events` to flush the in-memory event cache.
- Do not imply this deletes `/api/jobs/<job-id>/evidence` or artifact files.
- After flushing, reconnecting to `/api/ws` will not replay the cleared in-memory events, but persisted job evidence remains available.

9. If the user wants another attempt, use the rerun endpoint instead of reconstructing arguments manually when possible.
10. Prefer filtered event queries over downloading artifacts when the goal is quick debugging.
11. Report outcomes in operator language: device, action, job ID, current status, key evidence, and any next step.

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
- Do not invent payloads for `ssh.exec` or `serial.write`; ask one short question if the value is missing.
- For `ssh.tail`, ask only when the payload is missing and the device does not have `ssh.default_log_path` configured.
- Prefer using the rerun endpoint for repeat attempts so the same stored inputs are reused.
- If the backend is down, say so clearly and help restart or diagnose it before issuing device commands.
- Treat long-running tails as ongoing jobs; cancel them explicitly instead of assuming they stop on their own.
- Treat `serial.write` as a short-lived job that confirms bytes were sent, not as the place to inspect device output.
- Treat log cache flushing as an in-memory replay cleanup only; never describe it as evidence deletion.
- For AI-driven debugging, prefer concise API queries, filtered WebSocket streams, and filtered event endpoints over UI walkthroughs.

## Customization notes

Users can copy this skill folder and adjust only operator-facing details:

- Base URL if the service does not run on `3000`
- Supported action examples if the deployment exposes more or fewer actions
- Response format if they prefer more concise or more verbose operator output
