---
id: INDEX-013
title: Publish persistent location statistics events
status: In Progress
assignee: codex
priority: High
tags: [core, indexing, events, locations]
last_updated: 2026-07-31
---

## Description

Persistent indexing updates location scan state and aggregate statistics in the
library database. Publish the updated location through the normal resource event
path so subscribed clients observe the same file count, byte total, and scan
state returned by `locations.list`.

## Acceptance Criteria

- Persistent location record updates emit a `ResourceChangedBatch` event for the
  `location` resource after the database write succeeds.
- Enabling indexing emits the durable index-mode update before dispatch, while
  the indexer job remains the sole owner of the initial scanning transition.
- A failed job dispatch leaves the location scan state idle and retryable.
- The emitted location statistics match the values returned by `locations.list`.
- Focused indexing integration tests cover the completed indexing event and query
  convergence.
- Rust formatting and focused `sd-core` validation pass.

## Implementation Notes

Keep this task `In Progress` until the implementation is merged into `main`.
