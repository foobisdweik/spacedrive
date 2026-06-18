# Spacedrive Architecture Analysis Report

This report presents a comprehensive architectural mapping of the Spacedrive repository, achieved through recursive decomposition and targeted analysis of key source files across operations, domain modeling, extension APIs, and the frontend. 

## 1. Core Operations (`core/src/ops`)
The backend logic is structured tightly around a CQRS (Command Query Responsibility Segregation) pattern, encapsulated as `LibraryAction`, `CoreAction`, `LibraryQuery`, and `CoreQuery` types with associated registration macros.

### Locations Management (`core/src/ops/locations`)
- **LocationAddAction** (`add/action.rs:32`) - Registers `locations.add` (`:249`)
- **LocationRemoveAction** (`remove/action.rs:21`) - Registers `locations.remove` (`:73`)
- **LocationUpdateAction** (`update/action.rs:35`) - Registers `locations.update` (`:146`)
- **LocationImportAction** (`import/action.rs:22`) - Registers `locations.import` (`:355`)
- **LocationExportAction** (`export/action.rs:19`) - Registers `locations.export` (`:769`)
- **LocationRescanAction** (`rescan/action.rs:25`) - Registers `locations.rescan` (`:108`)
- **LocationTriggerJobAction** (`trigger_job/action.rs:59`) - Triggers location jobs (`:327`)
- **EnableIndexingAction** (`enable_indexing/action.rs:34`) - Controls indexer state (`:221`)
- **LocationsListQuery** (`list/query.rs:13`) & **SuggestedLocationsQuery** (`suggested/query.rs:14`)

### File Management (`core/src/ops/files`)
- **FileCopyAction** (`copy/action.rs:59`) - Copies files and handles background jobs (`:627`)
- **FileDeleteAction** (`delete/action.rs:17`) - Hard/soft deletes (`:93`)
- **FileRenameAction** (`rename/action.rs:19`) - Renames entries (`:101`)
- **CreateFolderAction** (`create_folder/action.rs:21`) - Folder provisioning (`:165`)
- **DuplicateDetectionAction** (`duplicate_detection/action.rs:18`) - Identifies clones (`:108`)
- **ValidationAction** (`validation/action.rs:16`) - Validates paths/files (`:99`)

### Library Management (`core/src/ops/libraries`)
- **LibraryCreateAction** (`create/action.rs:14`) - Provisions libraries (`:85`)
- **LibraryOpenAction** (`open/action.rs:16`) - Connects clients to library scope (`:126`)
- **LibraryExportAction** (`export/action.rs:12`) - Exports DB/metadata (`:108`)
- **LibraryDeleteAction** (`delete/action.rs:15`) - Removes library structure (`:79`)
- **LibraryRenameAction** (`rename/action.rs:22`) - Modifies library alias (`:95`)

### Spaces (`core/src/ops/spaces`)
- **SpaceCreateAction** (`create/action.rs:15`) - Defines new spatial views (`:117`)
- **AddGroupAction** (`add_group/action.rs:15`) - Space categorization (`:120`)
- **AddItemAction** (`add_item/action.rs:16`) - Binds files to spaces (`:155`)
- **SpaceUpdateAction** / **SpaceDeleteAction** - Mutable state (`update/action.rs:15`, `delete/action.rs:13`)

### Media & AI Generation (`core/src/ops/media`)
- **ThumbnailAction** (`thumbnail/action.rs:24`) & **RegenerateThumbnailAction**
- **GenerateProxyAction** (`proxy/action.rs:36`) - Fast media streaming (`:180`)
- **ExtractTextAction** (`ocr/action.rs:37`) - OCR job processing (`:105`)
- **TranscribeAudioAction** (`speech/action.rs:26`) - Local Whisper audio transcription (`:106`)
- **GenerateSplatAction** (`splat/action.rs:29`) - Gaussian splat generation (`:97`)
- **GenerateThumbstripAction** (`thumbstrip/action.rs:36`) - Timeline thumbnails (`:183`)

### Networking & Pairing (`core/src/ops/network`)
- **NetworkStartAction** / **NetworkStopAction** (`start/action.rs:5`, `stop/action.rs:5`)
- **PairGenerateAction** (`pair/generate/action.rs:6`) - Creates pairing payload
- **PairJoinAction** (`pair/join/action.rs:5`) & **PairVouchAction** (`pair/vouch/action.rs:6`)
- **LibrarySyncSetupAction** (`sync_setup/action.rs:10`) - Initializes P2P state machine
- **SpacedropSendAction** (`spacedrop/send/action.rs:5`) - Ephemeral peer transfers

## 2. Domain Models (`core/src/domain`)
The domain layer implements the core types and structures that the database and CQRS handlers rely on, enforcing data integrity and boundaries.

- **Space**, **SpaceGroup**, **SpaceItem**, **SpaceLayout** (`space.rs`) - Foundational taxonomy structures
- **Location**, **JobPolicies** (`location.rs`) - Local and cloud mount boundaries with scanning properties
- **Volume**, **VolumeFingerprint**, **ApfsContainer** (`volume.rs`) - Low-level physical volume representations
- **Tag**, **TagApplication**, **CompositionRule** (`tag.rs`) - Tagging and tag algebra systems
- **Device**, **ConnectionMethod** (`device.rs`) - System device tracking
- **File**, **Sidecar**, **EntryKind** (`file.rs`) - Virtual distributed file system (VDFS) data objects
- **VectorStore**, **Document**, **Fact**, **MemoryFile** (`memory/`) - Internal RAG and persistent vector layouts
- **ContentIdentity**, **ContentHashGenerator** (`content_identity.rs`) - Immutable hashing and identity 

## 3. Extension SDK (`crates/sdk`)
Provides the FFI-boundary safe types and traits to allow extensions (compiled to WASM) to safely interact with core capabilities.

- **AgentContext**, **WorkingMemory**, **TemporalMemory**, **JobDispatcher** (`agent.rs`)
- **AiContext**, **AiModelRegistry**, **ModelHandle**, **PromptBuilder** (`ai.rs`)
- **VdfsContext**, **EntryQuery**, **MappedQuery** (`vdfs.rs`)
- **ActionContext**, **ActionPreview**, **ExecutionResult** (`actions.rs`)

## 4. Desktop Client UI (`apps/tauri/src`)
Frontend components rely on React context hooks and invoke Tauri's IPC commands generated seamlessly from the Rust definitions.

- **Context Menus**: Native context menu generation (`ContextMenuWindow.tsx`, `contextMenu.ts`)
- **Drag & Drop Logic**: Drag configurations (`lib/drag.ts`) and overlay hook handlers (`useDragOperation.ts`, `useDropZone.ts`, `DragOverlay.tsx`)
- **Updaters**: Local deployment updater pipelines (`updater.example.ts`)
- **Keybinds**: Core keyboard interceptor logic spanning global application context (`keybinds.ts`)

---
**Methodology Note**: This report was compiled by filtering 1,577 source files down to the 147 most relevant core components based on Spacedrive terminology. Files were grouped into batches and analyzed by 10 parallel subagents to extract core structs, traits, macro registrations, and their corresponding file/line context.

## 5. Overview of Updating and Conversion to Finder Alternative

### Modernization & Catch-up (April 2025 -> Mid-2026)
Since the project has been stale since early 2025, bringing it to the bleeding edge requires:
1. **Rust Backend Upgrades**:
   - Bump `rust-version` from 1.81 to the latest stable (~1.9x).
   - Update core dependencies like `tokio` (from 1.42), `axum` (from 0.7.9), and `sqlx` (0.8) to their newest major releases to benefit from recent async/performance improvements.
   - Upgrade the `specta` bindings and Tauri ecosystem.
2. **Frontend Modernization**:
   - Update `react` from 19.1.0 to the current stable.
   - Update `bun`, `vite`, and `typescript` tooling to their latest versions.
3. **Audit Third-Party SDKs**:
   - Upstream APIs (like Whisper, FastEmbed, LanceDB) move fast. Models and embedding libraries need auditing for breaking API changes or newer binary distributions.

### Evolution into a "Real Alternative to Finder" (macOS Local File Management)
Currently, Spacedrive is heavily oriented around its VDFS (Virtual Distributed File System). To genuinely compete with Finder for local file management, it needs deep OS integration:

1. **Native macOS Hooks**:
   - **FileProvider Extension**: Integrate deeply via macOS `FileProvider` so Spacedrive's VDFS is natively mountable and behaves exactly like a native volume in standard OS file pickers.
   - **FSEvents Integration**: Shift away from manual/polled "rescanning" of locations (like `LocationRescanAction`) and instead utilize real-time native `FSEvents` to maintain absolute parity with the file system state.
2. **First-Class Performance on Local Storage**:
   - Bypassing the VDFS overhead for purely local file moves/copies (using native APFS cloning `clonefile` instead of chunk-by-chunk copy where possible).
   - Ensuring the background daemon's memory footprint is heavily optimized so it can run 24/7 without being noticeable.
3. **Feature Parity with Finder**:
   - Implementing native macOS-style tags (syncing database tags with `com.apple.metadata:_kMDItemUserTags` xattrs).
   - Adding missing classic views (Miller Columns, high-fidelity gallery view with native Quick Look).
   - Drag & drop fidelity that seamlessly bridges the webview and the native desktop.
