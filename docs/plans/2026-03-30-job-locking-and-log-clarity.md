# Job Locking And Log Clarity

## Summary

This change improves how operators inspect active jobs and protects devices from conflicting actions.

## Problem

- Job and live-log pages showed raw events, which made stdout, stderr, and system messages hard to distinguish during active runs.
- The server allowed overlapping actions on the same device, even when one running job should have exclusive control.
- Cancellation semantics for completed jobs were not fixed by API tests, which left room for accidental state regressions.

## Changes

- Add a shared frontend log event card that highlights channel, level, source, timestamp, device, and job identity with clearer visual grouping.
- Add server-side device locking for exclusive actions so conflicting requests return `409 Conflict` with the holding job ID.
- Reject cancellation of completed jobs with `409 Conflict` instead of mutating terminal job state.
- Add API integration coverage for device lock conflicts and completed-job cancellation behavior.

## Why

These changes make job execution easier to inspect from the UI, preserve exclusive access to hardware during active operations, and lock down API behavior for concurrency and cancellation edge cases.
