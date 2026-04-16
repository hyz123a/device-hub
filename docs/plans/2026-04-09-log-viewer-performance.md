# Log Viewer Performance

## Profile summary

- The main append bottleneck was DOM growth in stream mode: every chunk stayed mounted, so large static logs and long-running sessions paid a full layout and paint cost on every update.
- Popup-open latency was amplified by the same behavior because the initial render attempted to mount the full merged stream immediately.
- Active text selection could drift while new logs arrived because tail anchoring still restored scroll position during subsequent renders.

## Changes

- Increased the in-memory event ring to `5000` entries while trimming oldest logs once the cap is exceeded.
- Added chunk-window virtualization with top and bottom spacers so stream mode renders only the overscanned viewport slice.
- Kept scroll tracking explicit and froze viewport anchoring while the page has an active text selection.
- Added unit tests that cover large static logs, sustained streaming merges, and the new buffer/window sizing behavior.
- Extracted `StreamLogPanel` component from `LogViewer` to improve code organization and reusability.
- Added `Dockerfile` for containerized deployment.
