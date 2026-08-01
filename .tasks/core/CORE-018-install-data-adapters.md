---
id: CORE-018
title: Install data adapters from directories
status: In Progress
assignee: codex
priority: High
tags: [core, archive, adapters, interface]
last_updated: 2026-07-31
---

## Description

Expose the archive engine's adapter sideloading capability as a registered
library action and connect it to the Data Adapters directory picker.

## Acceptance Criteria

- `adapters.install` accepts a local adapter directory and returns the installed
  adapter ID.
- Adapter filesystem validation and copying use `Engine::install_adapter`.
- Blocking adapter installation runs outside the Tokio worker pool.
- The Data Adapters screen opens the native directory picker, reports failures,
  and refreshes the installed adapter list after success.
- Generated TypeScript bindings are regenerated.
- Rust compilation, TypeScript typechecking, formatting, and task validation
  pass.

## Implementation Notes

Keep this task `In Progress` until the implementation is merged into `main`.
