# Core Module

- Core architecture is CQRS/DDD: domain nouns and business rules are separated from operation verbs.
- Operations live under `core/src/ops/` and are registered through macros collected by inventory.
- Do not implement `Wire` manually. Use registration macros such as `register_query!`, `register_library_action!`, and `register_core_action!`.
- Operation registration/dispatch lives in both current paths around `core/src/ops/registry.rs` and newer infrastructure around `core/src/infra/wire/registry.rs`; inspect the local code before changing the boundary.
- Common operation module shape: `action.rs`, `input.rs`, `output.rs`, `job.rs` as applicable. Keep new features close to the existing domain module.
- Queries must not mutate state. Actions mutate state and may validate/confirm before execution.
- Long-running work belongs in durable jobs. Resumable jobs persist serializable state, skip runtime-only fields with `#[serde(skip)]`, checkpoint after meaningful progress, and remain idempotent across restarts.
- Generated client boundary:
  - Rust exposed types use Specta/type extraction.
  - Regenerate TypeScript with `cargo run --bin generate_typescript_types`; output is `packages/ts-client/src/generated.ts`.
  - Regenerate Swift with `cargo run --bin generate_swift_types`; output is under `packages/swift-client/Sources/SpacedriveClient/`.
  - Do not hand-edit generated bindings.
- Core docs worth reading before changes: `docs/core/architecture.mdx`, `docs/core/ops.mdx`, `docs/core/jobs.mdx`, `docs/core/testing.mdx`, and the domain doc matching the subsystem.
- Tests: unit tests live near implementation in `#[cfg(test)]`; core integration tests live under `core/tests/`. Use the subprocess framework for real multi-device networking and the mock/custom harness for deterministic sync/data logic.