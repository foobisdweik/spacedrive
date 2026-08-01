---
id: CORE-016
title: Persistent file favorites
status: In Progress
assignee: codex
priority: High
tags: [core, metadata, interface, favorites]
last_updated: 2026-07-31
---

## Description

Persist the Inspector favorite control as entry-scoped user metadata instead of
keeping the state only in React. Publish the updated file resource so Inspector
and Explorer clients converge on the durable value.

## Acceptance Criteria

- `metadata.set_favorite` is a registered library action that accepts an indexed
  entry UUID.
- Favoriting creates entry-scoped user metadata and unfavoriting reuses the same
  record.
- Updated metadata participates in library sync.
- File resources expose the persisted favorite value.
- The Inspector updates optimistically, reverts on failure, and follows resource
  updates.
- Focused Rust tests, TypeScript typechecking, formatting, and task validation
  pass.

## Implementation Notes

Keep this task `In Progress` until the implementation is merged into `main`.
