---
id: PLUG-002
title: Define and Implement VDFS Plugin API Bridge
status: Done
assignee: jamiepine
parent: PLUG-000
priority: High
tags: [plugins, wasm, api, vdfs, wire]
whitepaper: Section 6.8
last_updated: 2026-07-11
---

## Description

Define and implement the VDFS Plugin API bridge. This will be a secure, capability-based API that exposes a subset of the VDFS functionality to the sandboxed WASM plugins.

The key architectural insight: expose ONE generic `spacedrive_call()` function that routes to the existing Wire operation registry, reusing all daemon RPC infrastructure.

## Implementation Steps

1.  Design the API, focusing on security and a minimal set of capabilities.
    - Generic host function design complete
    - Routes through RpcServer::execute_json_operation()
2.  Implement the host-side functions that will be exposed to the WASM guest modules.
    - Skeleton exists in core/src/infra/extension/host_functions.rs
    - Needs: WASM memory interaction, full Wire bridge, error handling
3.  Implement the guest-side bindings for the API.
    - spacedrive-sdk with #[extension], #[job] macros
    - Beautiful API in extensions/test-extension/
4.  Ensure that plugins can only access the data and functionality they have been granted permission for.
    - Permission system in core/src/infra/extension/permissions.rs
    - Rate limiting included

## Remaining Work

- [x] Complete host_spacedrive_call() implementation
- [x] Add WASM memory read/write helpers
- [x] Connect to the Wire registry (same dispatch as RpcServer::execute_json_operation())
- [ ] Add extension-specific operations (ai.ocr, credentials.store, vdfs.write_sidecar)
- [x] End-to-end integration testing (bridge dispatch against a real Core; guest-load test exists)

## Acceptance Criteria

- [x] A clear API definition document is created.
- [x] A plugin can call a host function to interact with the VDFS (e.g., read a file) — `host_spacedrive_call` reads the call from WASM memory, checks permissions, and routes through `dispatch_extension_call` to the Wire registry; proven end-to-end against a real Core.
- [x] The API enforces the principle of least privilege.

## Implementation Notes (2026-07-11)

- Extracted the registry-dispatch core of `host_spacedrive_call` into a standalone `dispatch_extension_call(core_context, api_dispatcher, method, library_id, payload)` (`core/src/infra/extension/host_functions.rs`). It performs the same routing as `RpcServer::execute_json_operation`: try library-query → core-query → library-action → core-action, apply library context to the session for library-scoped ops, and return the operation's JSON (or an error string). `host_spacedrive_call` now reads method/library_id/payload from linear memory, checks permissions, and delegates to it — no behavior change, but the bridge routing is now unit-testable without a WASM guest.
- Tests: `core/src/infra/extension/host_functions.rs::bridge_tests` (4, `--features wasm`) — a core query returns a result, payload fields reach the operation (`libraries.list`), unknown methods are rejected, and a library-scoped method without a library id is rejected. Operations are addressed by their Wire method (`query:<name>` / `action:<name>`), the same strings daemon RPC and the SDK use.
- Confirmed the existing `wasm_extension_test` still loads a real guest with the `spacedrive_call` host import bound.

## Remaining (follow-up)

- Extension-specific operations (`ai.ocr`, `credentials.store`, `vdfs.write_sidecar`) are not yet registered — they are additional Wire operations on top of the now-working generic bridge.
- Full in-guest round trip (guest `.wasm` invoking `spacedrive_call` and reading the result back out of memory) is exercised manually; an automated guest-side test needs the test-extension rebuilt against the SDK with a call site.

## Implementation Files

- core/src/infra/extension/host_functions.rs - Host function skeleton
- core/src/infra/extension/permissions.rs - Capability-based security
- core/src/infra/extension/README.md - Architecture documentation
- extensions/spacedrive-sdk/ - Guest-side SDK (referenced)
