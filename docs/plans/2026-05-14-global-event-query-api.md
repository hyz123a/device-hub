# Global Event Query API

This change adds a global event query endpoint for inspecting the current in-memory log cache without first knowing a job id.

## Problem

Hardware debugging often starts from a device and a recent action, not from a specific job. Operators may need to answer questions like "did homebase emit reset logs in the last five minutes?" while a serial tail is already running. Before this change, `/api/events` only supported cache clearing, and event reads required a job-specific endpoint.

## Scope

- Add `GET /api/events` for the current server event cache.
- Support filters for `device_id`, `source`, `level`, `search`, `search_any`, `search_regex`, and `since`.
- Allow comma-separated device ids and keyword lists for multi-device and OR-style searches.
- Support absolute RFC3339 timestamps and relative windows such as `30s`, `5m`, and `2h`.
- Use Rust `regex` semantics for regex matching and return a clear bad request for invalid patterns.

## Outcome

Operators and AI agents can query recent device logs directly from the cache, combine broad filters with targeted text matching, and quickly inspect real hardware logs without reconstructing job context.
