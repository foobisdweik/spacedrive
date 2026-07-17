---
id: CORE-014
title: Cross-Platform File Opening Backend
status: Done
assignee: jamiepine
priority: High
tags: [core, platform, file-operations]
whitepaper: DESIGN-open-with.md
last_updated: 2026-07-15
related_tasks: [EXPL-004]
---

## Description

Implement the backend infrastructure for opening files with default and specific applications across macOS, Windows, and Linux. This ports v1's sophisticated platform-specific implementation to v2's architecture.

## Implementation Notes

Create platform-specific crates following v1's proven architecture:
- `apps/tauri/crates/file-opening/` - Shared types and traits
- `apps/tauri/crates/file-opening-macos/` - Swift via FFI using NSWorkspace APIs
- `apps/tauri/crates/file-opening-windows/` - COM Shell APIs (SHAssocEnumHandlers)
- `apps/tauri/crates/file-opening-linux/` - GTK/GIO with content type detection

Each platform implementation must:
1. Query OS for applications that can open a file
2. Return intersection of compatible apps for multi-file selection
3. Open file with default application
4. Open file(s) with specific application

See `DESIGN-open-with.md` for complete architecture details.

## Acceptance Criteria

- [x] Shared `file-opening` crate with `FileOpener` trait and types
- [x] macOS implementation using Swift FFI + NSWorkspace
  - [x] Query apps using `urlsForApplications(toOpen:)` API
  - [x] Filter to `/Applications/` directory
  - [x] Open with default via NSWorkspace
  - [x] Open with specific app by bundle ID
- [x] Windows implementation using COM Shell APIs
  - [x] Query apps using `SHAssocEnumHandlers`
  - [x] Thread-local COM initialization
  - [x] Open with default via ShellExecute
  - [x] Open with specific app via IAssocHandler
- [x] Linux implementation using GTK/GIO
  - [x] Content type detection from file magic bytes
  - [x] Query apps via `AppInfo::recommended_for_type`
  - [x] Open with default via `launch_default_for_uri`
  - [x] Open with specific app via DesktopAppInfo
- [x] Tauri commands registered:
  - [x] `get_apps_for_paths(paths)` - returns Vec<OpenWithApp>
  - [x] `open_path_default(path)` - returns OpenResult
  - [x] `open_path_with_app(path, app_id)` - returns OpenResult
  - [x] `open_paths_with_app(paths, app_id)` - returns Vec<OpenResult>
- [x] Intersection logic for multi-file selections works correctly
- [x] Error handling returns proper OpenResult variants
- [x] All commands are async and non-blocking

## Implementation Files

To be created:
- `apps/tauri/crates/file-opening/src/lib.rs`
- `apps/tauri/crates/file-opening/src/types.rs`
- `apps/tauri/crates/file-opening-macos/src/lib.rs`
- `apps/tauri/crates/file-opening-macos/src-swift/FileOpening.swift`
- `apps/tauri/crates/file-opening-windows/src/lib.rs`
- `apps/tauri/crates/file-opening-linux/src/lib.rs`
- `apps/tauri/src-tauri/src/commands/file_opening.rs`

To be modified:
- `apps/tauri/src-tauri/src/main.rs` (register commands and service)
- `apps/tauri/src-tauri/Cargo.toml` (add dependencies)

## Reference Implementation

v1 implementation can be found at:
- `~/Projects/spacedrive_v1/apps/desktop/src-tauri/src/file.rs`
- `~/Projects/spacedrive_v1/apps/desktop/crates/macos/src-swift/files.swift`
- `~/Projects/spacedrive_v1/apps/desktop/crates/windows/src/lib.rs`
- `~/Projects/spacedrive_v1/apps/desktop/crates/linux/src/app_info.rs`

## Testing

- Unit tests for intersection logic
- Platform-specific tests with mock file system
- Manual testing on macOS, Windows, Linux
- Test edge cases: no apps available, permission denied, file not found
