---
name: device-hub-user
description: Use the local Device Hub service as an API-first operator: inspect devices, run supported actions, review job evidence, and filter logs for debugging.
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
- Guide or perform common flows: list devices, inspect one device, start an action, watch job status, review evidence, filter logs, and rerun a job.
- Keep responses grounded in the actual service responses and supported actions.

## When to use me

- The user wants to use Device Hub as an operator rather than modify its code.
- The user asks to run or inspect device actions like `ssh.exec`, `ssh.tail`, `ssh.reboot`, `serial.tail`, or `serial.write`.
- The user wants to look at job evidence, filtered logs, timeout behavior, or rerun a previous job.
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
curl -s http://127.0.0.1:3000/api/devices
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

6. Filter job events for debugging

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

7. Download artifacts when needed

```bash
curl -OJ http://127.0.0.1:3000/api/jobs/<job-id>/artifacts/job.json
curl -OJ http://127.0.0.1:3000/api/jobs/<job-id>/artifacts/events.ndjson
```

8. Rerun a job with the same inputs

```bash
curl -s -X POST http://127.0.0.1:3000/api/jobs/<job-id>/rerun
```

9. Cancel a running job

```bash
curl -s -X DELETE http://127.0.0.1:3000/api/jobs/<job-id>
```

## Workflow

1. Confirm whether the user wants API execution, log inspection, or service troubleshooting.
2. Check backend availability before running actions.
3. If the task targets a device action, list devices first and verify the target device supports the requested action.
4. For actions that require payloads, confirm or infer the exact payload:

- `ssh.exec` requires a command string.
- `ssh.tail` requires a log path.
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
- `/api/jobs/<job-id>/events?source=...&level=...&search=...` for targeted debugging
- `/api/jobs/<job-id>/evidence` for summary and artifacts
- `/api/jobs/<job-id>/events` for full history

8. If the user wants another attempt, use the rerun endpoint instead of reconstructing arguments manually when possible.
9. Prefer filtered event queries over downloading artifacts when the goal is quick debugging.
10. Report outcomes in operator language: device, action, job ID, current status, key evidence, and any next step.

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
- Do not invent payloads for `ssh.exec`, `ssh.tail`, or `serial.write`; ask one short question if the value is missing.
- Prefer using the rerun endpoint for repeat attempts so the same stored inputs are reused.
- If the backend is down, say so clearly and help restart or diagnose it before issuing device commands.
- Treat long-running tails as ongoing jobs; cancel them explicitly instead of assuming they stop on their own.
- Treat `serial.write` as a short-lived job that confirms bytes were sent, not as the place to inspect device output.
- For AI-driven debugging, prefer concise API queries and filtered event endpoints over UI walkthroughs.

## Customization notes

Users can copy this skill folder and adjust only operator-facing details:

- Base URL if the service does not run on `3000`
- Supported action examples if the deployment exposes more or fewer actions
- Response format if they prefer more concise or more verbose operator output
