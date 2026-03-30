# Persist Job Output Artifacts

## Summary

This change persists completed job metadata and event streams on disk so operators can inspect historical runs even after the in-memory live event buffer rotates.

## Problem

- Job output only lived in the in-memory event buffer.
- Completed runs became harder to inspect once the live ring buffer rolled over or the server restarted.
- The UI did not tell operators where retained job output lived on disk before export support exists.

## Changes

- Add an artifact store under the configured artifact root that writes `jobs/<job-id>/job.json` and `jobs/<job-id>/events.ndjson`.
- Update job lifecycle handling so accepted, running, and completed state changes are persisted together with streamed job events.
- Add API support to read persisted job events when they are no longer available from the in-memory buffer.
- Surface the persisted artifact layout in the job detail view so operators can find retained job output.
- Add integration coverage for persisted job metadata and full event history.

## Why

Persisting job artifacts makes completed runs inspectable across buffer rotation and restarts, which improves diagnosis and keeps historical evidence available before dedicated export endpoints are added.
