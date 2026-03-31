# Serial Panel And Log Filters

## Summary

This change promotes serial input into a reusable frontend panel and adds focused event filtering so operators can debug active jobs without manually scanning the full event stream.

## Problem

- Serial write controls were tied to the device detail page instead of being reusable across the other log-heavy views.
- Job detail and log viewer pages exposed large event streams but did not offer enough focused filtering for source, severity, and message search.
- The operator skill documentation did not reflect the newer serial write and filtered event workflows.

## Changes

- Extract the serial write UI into a shared `SerialWritePanel` component and reuse it from the frontend pages that need serial interaction.
- Add richer job-event filtering in the server API and wire the frontend log views to those controls.
- Refresh the job detail, log viewer, and device detail pages so serial interaction and filtered event inspection fit the same evidence-first workflow.
- Update the operator skill documentation to match the supported API flows for serial write, filtered logs, evidence, and rerun.

## Why

Reusable serial controls and focused event filters make Device Hub easier to operate during hardware debugging sessions, especially when an operator needs to send serial input and isolate only the relevant subset of logs.
