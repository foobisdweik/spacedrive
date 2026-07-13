---
id: CLOUD-003
title: Cloud Storage Provider as a Volume
status: Done
assignee: jamiepine
parent: CLOUD-000
priority: High
tags: [cloud, storage, volume, s3]
whitepaper: Section 5.2
last_updated: 2026-07-12
---

## Description

Implement support for a cloud storage provider (e.g., S3-compatible service) as a native Spacedrive Volume. This will allow users to add cloud storage as a location in their library, just like a local disk.

## Implementation Steps

1.  Create a new `Volume` implementation for a generic S3-compatible API.
    - `VolumeBackend` trait implemented
    - `CloudBackend` with OpenDAL integration for S3-compatible services
    - Support for S3, R2, MinIO, Wasabi, Backblaze B2, DigitalOcean Spaces
2.  Implement the necessary file operations (read, write, list, delete) for the S3 API.
    - `read()`, `read_range()`, `write()`, `read_dir()`, `metadata()`, `exists()`
    - Sample-based content hashing using ranged reads (~58KB for large files)
3.  Integrate the new cloud volume type into the `VolumeManager`.
    - Cloud volumes tracked in database
    - Credentials encrypted with XChaCha20-Poly1305 and stored in OS keyring
    - `VolumeAddCloudAction` and `VolumeRemoveCloudAction` implemented
4.  Develop the CLI/UI flow for adding and configuring a cloud storage volume.
    - CLI commands: `sd volume add-cloud`, `sd volume remove-cloud`
    - Support for custom endpoints (R2, MinIO, etc.)
5.  Update query system to support cloud paths.
    - `Entry::try_from` supports `SdPath::Cloud`
    - `DirectoryListingQuery` supports cloud directories
    - `FileByPathQuery` supports cloud files
6.  Update indexer to use VolumeBackend for I/O operations.
    - Query layer supports cloud paths
    - Discovery phase uses backend.read_dir()
    - Processing phase handles cloud backends (skips change detection for cloud)
    - Content phase uses backend for content hashing

## Acceptance Criteria

- [x] A user can add an S3 bucket as a new location in their library.
- [x] Files can be copied to and from the cloud volume (via `CloudTransferStrategy`; upload/download/cloud-to-cloud through the `VolumeBackend` layer).
- [x] The cloud volume can be indexed like any other location.

## Implementation Notes (2026-07-11)

- Added `CloudTransferStrategy` (`core/src/ops/files/copy/strategy.rs`): a backend-agnostic copy strategy that resolves both endpoints to a `VolumeBackend` (`LocalBackend`/`CloudBackend`) and mirrors bytes between them. One implementation covers upload (local → cloud), download (cloud → local), and cloud → cloud. Files stream in 8 MiB chunks via `read_range` for progress reporting; directories recurse through `read_dir`. Optional `verify_checksum` reads the destination back and compares size.
- `CopyStrategyRouter` now routes to `CloudTransferStrategy` whenever either endpoint `is_cloud()` — this must run before the device/local checks, since cloud paths have no device slug or local path. `select_strategy`, `select_strategy_with_metadata`, and `describe_strategy` all handle cloud endpoints (upload/download/cloud-to-cloud descriptions).
- Endpoint resolution reuses `VolumeManager::resolve_volume_for_sdpath` + `backend_for_volume`; cloud paths address the backend by their in-bucket object key, local paths by absolute filesystem path.
- Added `CloudBackend::new_in_memory()` (OpenDAL in-memory service, new `services-memory` feature) so cloud code paths are testable without live S3.
- Tests: `core/src/ops/files/copy/strategy.rs::cloud_transfer_tests` (5) — upload, download+verify, chunked-progress on a 20 MiB file, recursive directory upload, and zero-byte transfer, exercising a real `CloudBackend` ↔ `LocalBackend`.
- Deferred: resumable/multipart uploads (currently buffers a file before writing, per the `VolumeBackend::write` whole-buffer contract) and content-hash (not size-only) checksum verification — revisit alongside large-object streaming.

## Implementation Notes (2026-07-12) — streaming multipart + checksum

- Added a streaming write primitive to the backend layer: `VolumeBackend::open_writer(path, size_hint)`
  returns a `VolumeWriter` (`write_chunk` + `close`) (`core/src/volume/backend/mod.rs`).
  - `CloudBackend` implements it via OpenDAL `writer_with(key).chunk(8 MiB)`, so services with
    native multipart upload (S3/GCS/Azure) stream the object in parts; backends without multipart
    commit once at close (`core/src/volume/backend/cloud.rs`).
  - `LocalBackend` implements it via a buffered `tokio::fs::File`, flushing and `sync_all` on close
    (`core/src/volume/backend/local.rs`).
- Rewrote `CloudTransferStrategy::transfer_file` to stream each 8 MiB source chunk straight into the
  destination writer — no whole-file `Vec` buffer (the previous `try_reserve(size)` allocation is gone),
  so object size is bounded by the destination, not RAM. The blake3 source hash is updated per-chunk,
  and the existing size + streamed destination read-back verification is preserved.
- Content-hash checksum verification (not size-only) had already landed in the 2026-07-11 hardening
  commit; it now runs over the streaming path.
- Tests: `cloud_transfer_tests` grew from 5 to 7 — a 20 MiB+tail transfer with `verify_checksum` that
  would have buffered the whole file before, and a corrupting-destination case proving the streamed
  checksum comparison catches same-length content corruption.
- Still deferred: crash-resume multipart (persisted upload id / parts) and OAuth cloud providers.

## Implementation Files

**Core Backend:**

- `core/src/volume/backend/mod.rs` - VolumeBackend trait
- `core/src/volume/backend/local.rs` - LocalBackend implementation
- `core/src/volume/backend/cloud.rs` - CloudBackend with OpenDAL

**Credential Management:**

- `core/src/crypto/cloud_credentials.rs` - CloudCredentialManager

**Actions:**

- `core/src/ops/volumes/add_cloud/` - VolumeAddCloudAction
- `core/src/ops/volumes/remove_cloud/` - VolumeRemoveCloudAction

**CLI:**

- `apps/cli/src/domains/volume/` - CLI commands

**Query System:**

- `core/src/domain/entry.rs` - Cloud path support
- `core/src/ops/files/query/directory_listing.rs` - Cloud directory browsing
- `core/src/ops/files/query/file_by_path.rs` - Cloud file lookup

## Next Steps

1. Test end-to-end cloud volume indexing with MinIO or real S3
2. ~~Implement file copy operations for cloud volumes~~ — done (`CloudTransferStrategy`, now streaming)
3. Add OAuth support for Google Drive, Dropbox, OneDrive
4. Crash-resume multipart uploads (persist upload id / uploaded parts)
