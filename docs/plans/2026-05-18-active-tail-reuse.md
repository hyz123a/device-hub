# Active Tail Discovery and Reuse

This change lets operators discover active tail sessions from device detail and explicitly reuse an existing serial tail instead of opening the same port again.

## Context

Serial log collection often runs for a long time while an operator or AI agent sends follow-up commands. Before this change, a caller that hit a serial port conflict had to manually scan `/api/jobs` to find the active `serial.tail` job. That was brittle and made the operator workflow slower.

## Changes

- Add `runtime.active_tails` to device detail responses with `action`, `job_id`, and `active_since`.
- Add `already_running` to action start responses while preserving the existing flattened job fields.
- Support `POST /api/devices/<device-id>/actions/serial.tail?reuse=true` so callers can opt into reusing an active serial tail.
- Keep normal action-start behavior unchanged when `reuse=true` is not provided.
- Update the device detail UI and operator skill documentation to use device runtime status before falling back to job inspection.

## Testing

- API coverage verifies active serial tail discovery through device detail.
- API coverage verifies `serial.tail?reuse=true` returns the active job without opening the serial port again.
- The frontend build is checked against the updated shared response types.
