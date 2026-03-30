# Tailwind Frontend Migration

## Summary

This change moves the Yew frontend onto generated Tailwind CSS utilities so the UI can evolve without maintaining a parallel handwritten stylesheet for every layout and state variant.

## Problem

- The frontend styling had grown into a large custom CSS file that mixed tokens, layout primitives, and page-specific rules.
- Yew pages needed a more scalable styling model, but the project still needs to keep Rust/Yew as the frontend runtime.
- The build needed a reproducible way to generate CSS from utility classes written in `.rs` files.

## Changes

- Add Tailwind CLI setup in `frontend/package.json` and `frontend/tailwind.config.js`.
- Configure Tailwind to scan `frontend/src/**/*.rs` so Yew `html!` markup produces the required utility classes.
- Wire `frontend/Trunk.toml` to regenerate the compiled stylesheet before frontend builds.
- Migrate the main Yew pages and shared components to utility-based styling.
- Document the Rust/Yew Tailwind approach in `docs/yew-tailwind-css.md`.

## Why

Tailwind keeps the Yew frontend styling easier to extend and refactor while preserving the current Rust-based architecture and making the generated CSS build reproducible for local development.
