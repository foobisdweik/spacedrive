# Tech Stack

- Rust workspace, edition 2021, workspace `rust-version = 1.96`; local toolchain file exists at `rust-toolchain.toml`.
- Rust async/runtime stack centers on Tokio. Runtime diagnostics use `tracing`/`tracing-subscriber`.
- Core data stack includes SQLite via SeaORM/sqlx, Specta for type extraction, Iroh/QUIC for P2P networking, BLAKE3/content identity, OpenDAL for cloud storage, LanceDB/FastEmbed for vector/search features, and optional media/AI features behind Cargo features.
- Workspace members include `core`, `apps/cli`, `apps/server`, `apps/tauri/sd-tauri-core`, `apps/tauri/src-tauri`, mobile core, `crates/*`, `core/benchmarks`, and `xtask`. Tauri is excluded from default workspace builds because it requires frontend assets.
- JS package manager is Bun. Root `package.json` pins `packageManager: bun@1.3.14`, requires Bun `>=1.3.0`, and Node `>=18.18 <19 || >=20.1`.
- Desktop frontend: Tauri 2, Vite, React 19, Tailwind CSS v4, shared SpaceUI packages.
- Mobile frontend: Expo/React Native, React 19, React Native 0.81.5, NativeWind/Tailwind v3.
- TypeScript client is generated from Rust via Specta into `packages/ts-client/src/generated.ts`; Swift client is generated into `packages/swift-client/Sources/SpacedriveClient/`.
- Task tracking is versioned Markdown/YAML under `.tasks/` and validated by the Rust `task-validator` crate.