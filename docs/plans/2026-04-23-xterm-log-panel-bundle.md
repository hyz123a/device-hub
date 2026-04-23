# Xterm Log Panel Bundle

## Summary

This change switches the live stream log panel to an embedded `xterm.js` terminal so operators can follow raw device output with incremental rendering, ANSI formatting, and terminal-style scrolling.

## Problem

- The previous merged chunk renderer rebuilt large HTML log sections during live updates, which made sustained device streams harder to follow and more expensive to render.
- WebSocket event filters existed for job event pages but not for live stream subscriptions, so the terminal view could not limit traffic to the active source or search term.
- The frontend did not bundle the terminal runtime as a reusable asset, which made the richer live log view unavailable to the server-served UI.

## Changes

- Add a dedicated frontend bundle for `xterm.js`, including the generated bridge assets and terminal setup code used by the Yew app.
- Replace the stream-mode chunk list with an `XtermTerminal` component that appends new log events incrementally and preserves terminal-style output.
- Extend the shared event filter and WebSocket API query handling so live subscriptions can filter by source, level, search text, and structured-only mode.
- Update integration and component tests to cover WebSocket stream filtering and the new terminal event formatting behavior.

## Why

Operators use Device Hub to debug long-running hardware sessions. A terminal-backed stream view keeps the live log experience closer to the real device output while the added WebSocket filters reduce noise and bandwidth for focused troubleshooting.
