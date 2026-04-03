# Host Device Scan For Setup Flows

## Summary

This change adds host-side discovery for serial character devices and connected J-Link probes so operators can configure targets from scan results while still keeping manual entry as the primary editing path.

## Problem

- The device form required operators to type serial port paths and J-Link probe details by hand even when those values were already discoverable on the host.
- Manual setup was error-prone because the UI could not show which serial ports or J-Link probes were actually present.
- Switching between a profile-backed J-Link selection and manual editing risked discarding a manually entered target device model.

## Changes

- Add server-side discovery endpoints for host serial ports and J-Link probes through explicit server seams instead of embedding adapter calls in API handlers.
- Extend the serial and J-Link adapter layer with discovery support that can be exercised through stable integration-test injection points.
- Update the device management page so operators can scan and choose a serial path or J-Link probe while keeping custom/manual entry available as the preferred mode.
- Preserve a manually entered J-Link device model when switching away from a scanned or profile-backed selection back to manual mode.
- Add tests covering discovery responses and the manual-first device setup behavior.

## Why

Device setup should reflect what the local lab host can actually see, but operators still need full manual control for boards, probes, and firmware targets that do not match a predefined profile. Keeping manual mode primary while layering scan-assisted selection on top reduces mistakes without boxing advanced users into preset-only flows.
