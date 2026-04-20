# Log Debug Defaults And Skill Templates

## Summary

- Improve frontend log viewing so sustained streams stay readable and auto-scroll stays reliable.
- Add a device-level SSH default log path so `ssh.tail` can start without requiring an operator to type a path every time.
- Keep device-specific recovery and workflow differences in skill templates instead of expanding the core product into per-device workflow logic.

## Why

The product's highest-priority capability is fast debugging against real devices. Operators and AI agents need to get to the right logs quickly, keep the live stream usable under load, and apply per-device debug conventions through reusable skills.

## Changes

- Increase the frontend buffered log default, improve tail-follow behavior, and make the log viewport more stable during heavy streaming.
- Extend SSH device configuration with `default_log_path`, defaulting to `/tmp/sdcard/debug_log` when omitted.
- Let `ssh.tail` fall back to the device default log path when the request payload is empty.
- Surface the default SSH log path in device management and explain the fallback behavior in the device detail page.
- Update operator-facing docs and skills so they reflect the new default-log-path behavior and the skill-template product direction.
