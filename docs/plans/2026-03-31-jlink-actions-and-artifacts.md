# J-Link Actions And Artifacts

## Summary

This change makes J-Link operations first-class jobs in Device Hub and preserves structured results plus raw SEGGER output as downloadable artifacts.

## Problem

- Devices could declare J-Link configuration, but the API and UI did not expose probe, flash, verify, or reset as normal jobs.
- The adapter mostly surfaced raw tool output, which made it harder to extract clear outcomes from successful and failed hardware operations.
- Evidence for J-Link runs was incomplete because command scripts and raw tool logs were not retained alongside job metadata.

## Changes

- Add J-Link action routing in the server and corresponding controls in the device detail page for `jlink.probe`, `jlink.flash`, `jlink.verify`, and `jlink.reset`.
- Extend the J-Link adapter to return structured execution results and attach raw command/log artifacts for each run.
- Persist J-Link artifacts through the job evidence pipeline so probe, flash, verify, and reset runs produce downloadable files.
- Update shared device schema and example configuration to carry the J-Link connection settings needed by the adapter.
- Add integration coverage for J-Link job lifecycle behavior and surfaced device capabilities.

## Why

Treating J-Link operations as normal jobs keeps firmware-oriented workflows observable, reproducible, and diagnosable through the same API and evidence model already used for SSH and serial actions.
