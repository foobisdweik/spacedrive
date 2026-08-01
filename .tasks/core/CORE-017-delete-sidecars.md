---
id: CORE-017
title: Delete generated sidecars
status: In Progress
assignee: codex
priority: High
tags: [core, sidecars, interface, files]
last_updated: 2026-07-31
---

## Description

Replace the Inspector sidecar deletion stub with a registered library action.
Delete managed sidecar files and records while preserving original files tracked
as reference sidecars.

## Acceptance Criteria

- `sidecar.delete` validates the requested kind and variant and reports missing
  sidecars.
- Managed sidecar files are removed with their database records.
- Reference sidecars remove only Spacedrive metadata and never delete the source
  file.
- Deletions participate in library sync and emit affected file resources.
- The Inspector reports success and failure to the user.
- Focused Rust tests, TypeScript typechecking, formatting, and task validation
  pass.

## Implementation Notes

Keep this task `In Progress` until the implementation is merged into `main`.
