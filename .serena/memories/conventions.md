# Conventions

- Architecture: daemon/client, local-first VDFS, CQRS, and DDD-style boundaries. Domain data/business rules are nouns; operations/jobs are verbs.
- Follow existing module structure before adding abstractions. Feature operation modules usually contain `action.rs`, `input.rs`, `output.rs`, and `job.rs` only when needed.
- Registration: never hand-write `Wire` implementations for normal operations. Use `register_query!`, `register_library_action!`, and `register_core_action!` macros.
- Public/cross-language boundaries require reference checks across enabled languages before changes: Rust API/type extraction, generated TS, generated Swift, app callers, and FFI/bridge code when relevant.
- Frontend code must use generated types from `packages/ts-client`; do not redefine backend request/response shapes and do not use `any` to bypass type errors.
- Generated outputs are not edited by hand. Regenerate with the relevant Rust generator binary.
- Rust imports are grouped std, external crates, then local modules with blank lines between groups.
- Rust naming: functions/variables `snake_case`, types/traits `PascalCase`, constants `SCREAMING_SNAKE_CASE`.
- Error handling: prefer typed errors with `thiserror` at library/domain boundaries; use `anyhow` where concrete public error type is unnecessary; propagate with `?` and preserve context at subsystem boundaries.
- Async: use Tokio primitives and `tokio::fs` in async paths; use `spawn_blocking` for unavoidable blocking or CPU-heavy work.
- Logging: use `tracing`, not `println!`/`eprintln!`; use structured fields where useful. In jobs, prefer job context logging so job metadata is attached.
- Job progress must reflect durably completed work when restart/resume could otherwise duplicate or lose data.
- Comments should explain rationale, invariants, platform behavior, or non-obvious fallbacks. Avoid placeholder comments and comments that restate code.
- Task files belong under `.tasks/` only for features, significant refactors, architecture/design work, or implemented specs. Do not create tasks for trivial fixes, formatting, routine docs, or dependency bumps.