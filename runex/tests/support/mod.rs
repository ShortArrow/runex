//! Shared test harness for runex integration tests.
//!
//! Imported by each `tests/*_integration.rs` via `mod support;`.
//! Rust's integration-test layout treats files under `tests/` as
//! separate crates, but a directory whose entry point is `mod.rs`
//! is *not* compiled as its own test binary — so this module is
//! shared source that each test crate compiles into itself.
//!
//! Two submodules:
//! - `subprocess`: cross-platform helpers (bin path, config writes,
//!   "is this shell installed" probes). Used by every shell test.
//! - `pty`: expectrl-backed PTY session wrapper. Unix only; gated
//!   behind `cfg(target_family = "unix")` to match the dev-dep
//!   declaration in `runex/Cargo.toml`.

#![allow(dead_code)] // each test crate uses a subset; quiet the per-crate warnings

pub mod subprocess;

#[cfg(target_family = "unix")]
pub mod pty;
