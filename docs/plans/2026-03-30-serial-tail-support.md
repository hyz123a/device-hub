# Serial Tail Support

## Summary

This change makes `serial.tail` a first-class job across the shared schema, server action routing, serial adapter, and Yew UI so operators can stream serial logs through the same job model used by SSH actions.

## Problem

- Devices could declare serial configuration, but serial log streaming was not available as a structured action.
- The server job runner only handled SSH actions, so serial streaming did not participate in the normal job lifecycle, websocket events, or cancellation flow.
- The device detail page and log views did not expose serial-tail actions as part of the primary workflow.

## Changes

- Add `SerialTail` action capability and serial configuration support to the shared schema and device summaries.
- Introduce a `device-hub-adapter-serial` crate that opens serial ports asynchronously, emits bounded log chunks, and supports task cancellation through the existing job runtime.
- Extend server action routing and job execution so `serial.tail` starts a normal job, streams unified `LogEvent` records, and behaves like other tail jobs.
- Update the frontend device and job pages to expose serial configuration, launch and stop `serial.tail`, and label serial log events clearly.
- Add API and adapter tests covering serial action routing, device summaries, and streamed job behavior.

## Why

Making serial streaming a first-class job keeps hardware log collection inside the same observable, cancellable, and API-driven control plane as the existing SSH workflows.
