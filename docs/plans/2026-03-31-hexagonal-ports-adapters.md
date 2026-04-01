# Hexagonal Ports And Adapters Refactor

## Summary

This change restructures the server around ports, use cases, and driven adapters so the job runtime stays focused on orchestration while concrete device execution lives behind replaceable boundaries.

## Problem

- HTTP handlers still carried application validation and job-start logic that should live in a use-case layer.
- The job manager knew too much about concrete SSH, serial, and J-Link adapter implementations.
- Tests needed a cleaner seam for injecting behavior without coupling directly to adapter crates.

## Changes

- Add `server/src/use_cases.rs` so API handlers delegate device lookup, action validation, start, cancel, and rerun behavior through explicit application use cases.
- Add `server/src/ports.rs` to define the action-execution boundary the job runtime needs from external adapters.
- Introduce `server/src/services/adapter_action_runner.rs` as the driven adapter that translates job requests into concrete SSH, serial, and J-Link execution.
- Narrow `server/src/services/job_manager.rs` so it focuses on lifecycle orchestration, locks, timeout policy, and persistence instead of direct adapter details.
- Update app wiring and tests to compose the concrete adapters at the composition root and inject behavior through server-level seams.

## Why

This refactor makes the core server workflow easier to evolve, test, and extend by keeping dependency direction pointed at abstractions instead of concrete device adapters.
