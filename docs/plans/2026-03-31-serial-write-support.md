# Serial Write Support

## Summary

This change adds `serial.write` as a first-class job so operators can send serial input through Device Hub without leaving the same observable job and evidence flow used for tailing and other device actions.

## Problem

- `serial.tail` made serial output visible, but there was no supported way to send input back to the device.
- Serial input and serial tailing could not safely share the same active port session, which made combined observe-and-interact workflows awkward.
- The device page did not expose a simple path for sending newline-terminated commands or control input over serial.

## Changes

- Add `serial.write` to the shared action capability model and server action handling.
- Extend the serial adapter with shared serial sessions so active tail jobs can keep streaming while write jobs reuse the same port writer.
- Add bounded flush behavior so partial serial output remains observable even when writes happen between line breaks.
- Expose a basic serial input form in the device detail page to submit serial write jobs from the UI.
- Add API and integration coverage for serial write validation, serial session sharing, and end-to-end serial job behavior.

## Why

Adding serial input closes the loop between observing a device and interacting with it, which makes Device Hub more useful for bootloader flows, shell prompts, and hardware bring-up sessions.
