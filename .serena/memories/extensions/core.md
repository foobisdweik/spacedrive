# Extensions Module

- Extensions are sandboxed WASM modules built against `crates/sdk/` and `crates/sdk-macros/`; prefer SDK/proc macros over direct host FFI unless the task is specifically about the FFI layer.
- Example extension code lives under `extensions/test-extension/`; extension docs live under `docs/extensions/`.
- Build extensions with `cargo build --target wasm32-unknown-unknown --release` after installing the target with `rustup target add wasm32-unknown-unknown`.
- Expected artifact path is `target/wasm32-unknown-unknown/release/<extension_name>.wasm`.
- Extension system is still under development. Verify actual implementation and docs before relying on planned capabilities from introduction/design docs.
- Extension host calls route through the existing Wire/registry infrastructure; permission checks are method/prefix based.