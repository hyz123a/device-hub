# Job Evidence And Rerun

## Summary

This change makes jobs easier to diagnose and repeat by adding structured evidence, artifact downloads, rerun support, and explicit timeout-driven teardown behavior.

## Problem

- Job detail pages exposed raw events but did not summarize what a job requested, how it ended, or which retained artifacts were available.
- Persisted artifacts could be written to disk, but there was no API or UI flow to download them directly.
- Repeating a previous job required manually re-entering device, action, and arguments.
- The job runner did not surface per-action timeout policy in a way that explained teardown behavior for failed or cancelled jobs.

## Changes

- Add shared `JobEvidence`, `JobArtifact`, and `JobSummary` models to describe retained job output in a structured way.
- Add server endpoints for job evidence, artifact downloads, and rerunning a previous job with the same device, action, and payload.
- Extend artifact storage and job execution so evidence can include artifact lists, timeout-related outcome details, and preserved event history.
- Update the Yew job detail page to render summaries, download links, richer log context, and a rerun action.
- Add server and integration coverage for evidence retrieval, artifact downloads, rerun behavior, and timeout handling.

## Why

These changes tighten the harness-debug loop by making completed jobs easier to understand, preserving actionable evidence, and letting operators or AI rerun the same request without rebuilding the job by hand.
