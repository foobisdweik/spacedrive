---
id: JOB-004
title: Reactive job lifecycle and history clearing
status: In Progress
assignee: codex
parent: JOB-000
priority: High
tags: [jobs, daemon, interface, events]
last_updated: 2026-07-31
---

## Description

Keep ephemeral background indexing work visible through its full lifecycle,
provide a safe way to clear terminal job history, and avoid treating a closed
event subscription as proof that the daemon process stopped.

## Acceptance Criteria

- Background indexing jobs emit progress and terminal lifecycle events.
- `jobs.clear` deletes only completed, failed, and cancelled persisted jobs.
- Active and paused jobs remain untouched when history is cleared.
- The Jobs screen exposes cancelled jobs and a clear-finished control.
- The client refetches job history after a successful clear.
- A daemon-disconnected event is confirmed against live daemon status before
  the global disconnected overlay appears.
- Rust formatting and compilation, generated TypeScript types, interface
  typechecking, and task validation pass.

## Implementation Notes

Keep this task `In Progress` until the implementation is merged into `main`.
