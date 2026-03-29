# Contributing to runex

## Development

### Prerequisites

- Rust (stable)
- PowerShell 7+ (`pwsh`) — only required on Windows for the pwsh integration tests; tests are skipped on other platforms

### Build

```bash
cargo build
```

### Test

```bash
cargo test
```

## Releasing

### Version bump

Update the version in 3 places:

1. `runex-core/Cargo.toml` — `version`
2. `runex/Cargo.toml` — `version`
3. `runex/Cargo.toml` — `runex-core = { version = "..." }` dependency

All 3 must match. Run `cargo test` after bumping to verify the workspace builds cleanly.

### Versioning policy

- `0.x.y` — current phase; no stability guarantees
- Bump patch (`0.1.x`) for bug fixes, docs, and additive features
- Bump minor (`0.x.0`) for breaking changes to CLI interface or config schema

### Publishing

```bash
# Embed the current commit hash for cargo install users
RUNEX_GIT_COMMIT=$(git rev-parse --short=12 HEAD) cargo publish -p runex-core
RUNEX_GIT_COMMIT=$(git rev-parse --short=12 HEAD) cargo publish -p runex
```

`runex-core` must be published before `runex`.
