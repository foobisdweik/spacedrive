# Spacedrive Core v2 Agent Guide

This file defines repository-specific rules for coding agents. Prefer these instructions over generic defaults when they conflict.

## Repository Orientation

Spacedrive uses a daemon-client architecture. The daemon owns core state and services. CLI, web, and desktop clients communicate with it through registered operations.

Key locations:

- `src/domain/`: domain models and business rules
- `src/ops/`: queries, actions, and jobs
- `core/src/ops/registry.rs`: operation registry
- `apps/tauri/`: primary desktop app
- `packages/ts-client/`: generated TypeScript client
- `packages/swift-client/`: generated Swift client
- `crates/sdk/`: extension SDK
- `crates/sdk-macros/`: extension procedural macros
- `extensions/test-extension/`: extension example
- `/docs/`: architecture and feature documentation
- `/.tasks/`: version-controlled task files

Before changing an unfamiliar subsystem, read the relevant `.mdx` or Markdown documentation under `/docs/`.

## Code Exploration and Editing

Use Serena for semantic exploration and structured edits when the active project supports the language.

Before reading an entire source file:

1. Run `get_symbols_overview`.
2. Use `find_symbol` with `include_body=false` to locate relevant symbols.
3. Retrieve only the symbol bodies needed for the task.
4. Run `find_referencing_symbols` before changing a public interface.

Prefer these Serena operations for structural edits:

- `rename_symbol`
- `replace_symbol_body`
- `insert_before_symbol`
- `insert_after_symbol`

Use grep or text search for strings, configuration files, generated artifacts, or unsupported languages.

Before changing a cross-language interface, identify references in every enabled language and inspect the relevant boundary, such as C ABI, C++ headers, Swift bridging headers, generated bindings, P/Invoke declarations, or FFI wrappers.

Do not modify build products, dependency caches, vendored code, or generated source directly.

## Validation Rules

After editing, run the narrowest formatter, compiler, linter, and test target that covers the change. Expand to broader validation only when needed.

Common commands:

```bash
cargo build
cargo test
cargo test <test_name>
cargo test --lib
cargo clippy
cargo fmt
cargo run --bin sd-cli -- <command>
```

The CLI binary is `sd-cli`, not `spacedrive`.

When daemon code changes:

```bash
cargo build
cargo run --bin sd-cli -- restart
```

For local daemon development:

```bash
cargo run --bin sd-daemon
```

Do not claim a change is complete unless the relevant validation ran successfully or you clearly state what could not be run.

## Architecture Rules

Spacedrive follows CQRS and domain-driven organization:

- Domain code contains core data structures and business rules.
- Queries read state without changing it.
- Actions change state.
- Jobs handle long-running or resumable work.

Feature modules normally live under `src/ops/` and may contain:

```text
feature/
├── action.rs
├── input.rs
├── output.rs
└── job.rs
```

Follow the existing module structure near the code being changed. Do not introduce a new abstraction when a local pattern already exists.

## Operation Registration

Never implement `Wire` manually. Use the registration macros:

```rust
crate::register_query!(NetworkStatusQuery, "network.status");
crate::register_library_action!(FileCopyAction, "files.copy");
crate::register_core_action!(LibraryCreateAction, "libraries.create");
```

The `inventory`-based registry collects registered operations automatically. Do not add manual registry entries unless the existing implementation explicitly requires it.

## Frontend and Generated Types

Frontend code must use the generated types from `packages/ts-client/`.

Do not:

- cast values to `any` to bypass type errors
- redefine backend request or response types manually
- duplicate generated client types in application code

When a Rust type exposed to the frontend changes, regenerate TypeScript bindings:

```bash
cargo run --bin generate_typescript_types
```

Generated output:

```text
packages/ts-client/src/generated.ts
```

For native prototypes, regenerate Swift bindings when an exposed interface changes:

```bash
cargo run --bin generate_swift_types
```

Generated output:

```text
packages/swift-client/Sources/SpacedriveClient/
```

Do not hand-edit generated bindings.

## Tauri Development

The primary desktop application is under `apps/tauri/`.

```bash
bun install
cd apps/tauri
bun run tauri:dev
bun run tauri:build
```

The development command starts the daemon automatically. Avoid running duplicate daemon instances unless the task requires it.

## Rust Standards

### Imports

Group imports in this order, separated by blank lines:

1. standard library
2. external crates
3. local modules

### Naming

- functions and variables: `snake_case`
- types and traits: `PascalCase`
- constants: `SCREAMING_SNAKE_CASE`

### Error Handling

Use typed errors for library or domain boundaries and contextual application errors where appropriate.

- Prefer `thiserror` for custom error enums.
- Use `anyhow` only where a concrete public error type is unnecessary.
- Propagate errors with `?` instead of swallowing them.
- Preserve useful context at subsystem boundaries.

### Async Code

- Use Tokio primitives in async code.
- Do not block the async runtime with synchronous filesystem or network I/O.
- Prefer `tokio::fs` over `std::fs` inside async paths.
- Use `tokio::task::spawn_blocking` for blocking or CPU-heavy work that cannot be made asynchronous.

### Logging

Use `tracing`, never `println!` or `eprintln!`, for runtime diagnostics.

```rust
use tracing::{debug, error, info, warn};

info!(port, "server started");
debug!(file_id = %id, "processing file");
warn!(error = %err, "retrying operation");
error!(error = %err, "operation failed");
```

Use structured fields where possible.

In job implementations, use the job context logger so messages include job metadata:

```rust
ctx.log().info("job started");
```

Log levels:

- `debug`: diagnostic execution detail
- `info`: meaningful lifecycle events
- `warn`: recoverable failures or fallbacks
- `error`: failures requiring attention

## Jobs

Jobs that can resume must persist enough state to continue safely after interruption.

- Store resumable state in serializable fields.
- Mark runtime-only fields with `#[serde(skip)]`.
- Check for interruption at practical boundaries.
- Checkpoint after meaningful progress.
- Make repeated execution idempotent where possible.

Do not report progress that has not been durably completed when a restart could cause duplication or data loss.

## Extension System

Extensions are sandboxed WASM modules built against the SDK in `crates/sdk/`.

Build an extension with:

```bash
cargo build --target wasm32-unknown-unknown --release
```

Expected output:

```text
target/wasm32-unknown-unknown/release/<extension_name>.wasm
```

Use the SDK and its procedural macros instead of calling host FFI directly unless the task is specifically about the FFI layer.

The extension system is still under development. Verify current behavior in the implementation and docs before relying on planned capabilities.

## Documentation and Comments

Explain why a decision exists, not what obvious code does.

Good comments describe rationale, invariants, fallback behavior, or platform consequences. Delete comments that only restate the next line.

Avoid:

- placeholder comments such as “for now”
- stale TODO comments for work that belongs in an issue or task
- section-divider comments
- comments describing removed code
- long prose where a clear name or helper function would be better

Module and public API documentation should include examples only when they clarify actual use. Keep examples compilable when practical.

## Formatting and Writing

Run `cargo fmt` before completing Rust changes.

For documentation, comments, and design text:

- use clear, direct sentences
- prefer active voice
- keep claims specific and verifiable
- avoid filler, metaphors, slogans, and promotional language
- do not use emojis

Do not enforce stylistic rules that make technical writing less precise.

## Testing

Place unit tests near the implementation in `#[cfg(test)]` modules. Place integration tests in the crate-level `tests/` directory.

Add or update tests when behavior changes, especially for:

- public operations
- serialization formats
- resumable jobs
- error paths
- cross-platform behavior
- generated client boundaries

Prefer focused tests for the modified behavior before running the full suite.

## Task Tracking

Use `/.tasks/` for work that introduces a feature, performs a significant refactor, changes architecture, or implements a design specification.

Do not create task files for formatting-only changes, routine documentation edits, dependency bumps, or trivial fixes.

Task filenames use:

```text
CATEGORY-###-title-slug.md
```

Validate task files with:

```bash
cargo run -p task-validator -- validate
```

Useful queries:

```bash
cargo run -p task-validator -- list --assignee "yourname" --status "In Progress"
cargo run -p task-validator -- list --priority "High" --sort-by id
```

See `/docs/core/task-tracking.md` for the schema and lifecycle.

## Common Failure Modes

Avoid these recurring mistakes:

- invoking `spacedrive` instead of `sd-cli`
- forgetting to restart the daemon after rebuilding it
- using `println!` instead of `tracing`
- implementing `Wire` manually
- blocking Tokio worker threads
- editing generated bindings directly
- bypassing frontend types with `any`
- changing public interfaces without checking all references
- running the full repository test suite when a narrow target would establish correctness faster
- treating planned extension behavior as implemented behavior

## Completion Checklist

Before finishing a task:

1. Review the diff for unrelated changes.
2. Confirm generated files were regenerated rather than edited manually.
3. Run the narrowest applicable formatter, linter, build, and tests.
4. Restart the daemon when required.
5. Update relevant task or documentation files when the change affects them.
6. Report validation performed and any remaining uncertainty.
