# Device Hub Story

## One-Line Product Story

Device Hub is a local-first device control plane that gives AI agents a safe, structured runtime for harness coding against real hardware.

## Why This Exists

AI can write test harness code, but it usually cannot reliably operate the real device world around that code.

Typical failures today:

- flashing is done by hand or with ad hoc scripts
- logs are scattered across SSH sessions, serial terminals, and probe tools
- device state is unclear after a failed run
- retrying a test requires manual cleanup and setup
- AI can generate harness code, but cannot safely execute, observe, and iterate on it

Device Hub exists to close that gap.

## Core Problem

For hardware-in-the-loop and device end-to-end testing, the hard part is not only writing tests. The hard part is turning real device operations into stable, repeatable, machine-callable actions.

That means AI needs more than shell access. It needs a harness runtime with:

- structured device actions
- unified logs and events
- job lifecycle tracking
- safe concurrency and locking
- reproducible execution context

## Product Positioning

Device Hub is not primarily:

- a browser terminal
- a generic remote shell tool
- a single-vendor flashing utility

Device Hub is:

- a device test control plane
- an AI-callable harness execution layer
- a local service for flashing, reset, command execution, and log collection

## Who It Is For

- AI agents generating and refining test harnesses
- engineers validating embedded or edge-device workflows
- teams running end-to-end tests against real hardware
- labs that need repeatable device setup, execution, and evidence capture

## Main User Journey

1. register a device with SSH and J-Link connection info
2. ask AI to create or update a harness
3. AI calls Device Hub to flash firmware, reset the target, and start logs
4. AI runs harness steps through SSH or other adapters
5. Device Hub streams structured logs, status, and artifacts back
6. AI analyzes failure evidence and revises the harness
7. the loop repeats until the harness stabilizes

## MVP Scope

The first version should prove one thing: an AI agent can complete a repeatable real-device harness loop.

MVP capabilities:

- device registry
- SSH adapter
  - exec
  - reboot
  - log follow
- generic J-Link adapter
  - probe
  - flash
  - reset
  - verify
- job model
- unified log/event stream
- artifact capture for job output
- simple Web UI for devices, jobs, and live logs

## Why This Matters for End-to-End Testing

For pure software E2E, many existing tools already work well.

For device E2E, teams still struggle with:

- restoring a known device state before each run
- collecting enough evidence after failure
- coordinating exclusive hardware access
- making execution understandable to both humans and AI

Device Hub directly addresses those gaps.

## Design Principles

- local-first: hardware control stays close to the host machine
- structured over interactive: actions are APIs, not terminal transcripts
- observable by default: every operation emits logs and status
- AI-safe: avoid raw unrestricted shell as the primary abstraction
- incrementally extensible: start with SSH and generic J-Link, add more adapters later

## Future Direction

Once the MVP works, Device Hub can expand into:

- richer harness asset management
- reusable test workflows
- serial and RTT log integration
- multi-device orchestration
- CI and remote lab execution
- stronger AI skills for autonomous diagnosis and recovery

## Success Criteria

The product is valuable if it can make this loop reliable:

- AI writes harness code
- Device Hub executes it against real hardware
- logs and artifacts are captured automatically
- failures are diagnosable
- the harness can be refined and rerun with minimal human intervention
