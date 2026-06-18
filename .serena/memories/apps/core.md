# Apps Module

- `apps/tauri/` is the primary desktop app. Run from that directory with Bun/Tauri; the dev command starts the daemon path for desktop development.
- Desktop package scripts:
  - `bun run tauri:dev` for development.
  - `bun run tauri:build` for bundled app builds; the script also runs daemon entitlement fixing for macOS release output.
  - `bun run typecheck` runs `tsc -b` for the Tauri app.
- `apps/tauri/src-tauri/` contains the Tauri 2 Rust app binary named `Spacedrive`; `apps/tauri/sd-tauri-core/` bridges desktop UI to core behavior.
- `apps/cli/` owns the `sd-cli` binary. Repo instructions explicitly prefer `cargo run --bin sd-cli -- <command>` and warn not to invoke `spacedrive` for CLI work.
- `apps/server/` owns `sd-server`, a headless HTTP/RPC server with an embedded daemon. It exposes health/RPC endpoints and is aimed at NAS/headless deployments.
- `apps/mobile/` is Expo/React Native. `apps/mobile/modules/sd-mobile-core/core` embeds Rust core for mobile.
- Frontend app code must consume generated types from `packages/ts-client`; do not duplicate backend request/response types or cast to `any` to bypass generated types.
- Shared UI lives in `packages/interface/`; check local component and route conventions before adding app UI.