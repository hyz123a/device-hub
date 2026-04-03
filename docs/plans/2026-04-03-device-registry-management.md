# Device Registry Management

## Summary

This change turns the device list into a persisted registry that operators can manage through the API and Web UI without losing the existing `devices.yaml` bootstrap path.

## Problem

- Devices were still effectively read-only because the server only loaded startup config into an in-memory map.
- Operators had no supported way to create, edit, or remove devices through the product itself.
- Once a persisted registry file existed, an empty file could hide the devices declared in `devices.yaml` and make the UI appear unconfigured.

## Changes

- Add a device-registry port plus a file-backed registry implementation that persists device CRUD changes to a dedicated YAML file.
- Add server-side device creation, update, and delete endpoints with validation for SSH, serial, and J-Link settings.
- Add a registry management experience on the devices page so operators can create, edit, and remove targets from the browser.
- Protect registry edits from clobbering active jobs and add regression coverage for CRUD behavior, persistence across restarts, and fallback seeding from `devices.yaml` when the persisted registry is empty.
- Ignore the generated `.device-registry.yaml` file in git so local registry state does not leak into source control.

## Why

Device Hub is meant to be the control plane for real hardware workflows, so device onboarding and maintenance need to be part of the product surface instead of a manual config-file step. Preserving `devices.yaml` as the initial seed also keeps existing setups working while the persisted registry becomes the new source of truth for ongoing edits.
