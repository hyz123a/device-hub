#

## User stroy

@./docs/device-hub-story.md

## Project architecture

@./docs/device-hub-architecture.md

### Implementation guardrails

- `server/src/api` is the inbound adapter layer and should call use cases instead of embedding business rules or concrete device execution details.
- `server/src/use_cases.rs` owns application actions such as start, cancel, and rerun.
- `server/src/services/job_manager.rs` owns job lifecycle orchestration and must depend on ports, not `device-hub-adapter-*` crates.
- Concrete adapter crates belong in the composition root and driven-adapter layer, currently `server/src/app.rs` and `server/src/services/adapter_action_runner.rs`.

## TDD

测试即文档

@./docs/device-hub-tdd-guide.md

### Test guardrails

- Integration tests should inject behavior through `server` seams and ports, not by directly depending on `device-hub-adapter-*` crates.
- When a new test needs a custom external behavior, add or extend a stable `server`-level injection seam first.
