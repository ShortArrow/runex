//! Internal library entry point — currently a placeholder.
//!
//! Phase C scaffold. The bin's `main.rs` still owns every `mod`
//! declaration today; this `lib.rs` exists so `[lib]` in
//! `runex/Cargo.toml` has a target to point at, and so the
//! crate-level "not a public API" disclaimer has a single
//! canonical location.
//!
//! After Phase C completes (when `runex-core` is absorbed and the
//! `domain` / `app` / `infra` directories actually contain modules),
//! this file will switch to `pub mod` declarations so
//! `runex/tests/*.rs` can `use runex::domain::expand::expand;`
//! without going through `runex-core`.
//!
//! # Not a public API
//!
//! Items behind `pub` (when there are any) are renamed,
//! restructured, or removed without notice across patch releases.
//! Semver guarantees apply only to the `runex` CLI's user-facing
//! surface:
//!
//! - The `config.toml` schema (documented in
//!   `docs/config-reference.md`).
//! - The `runex hook` per-keystroke RPC output format (line / cursor
//!   directives the shell wrappers `eval`).
//! - The `runex doctor --json` output: a top-level array of check
//!   objects, each with at least `name` (string) and `status` (one
//!   of `"ok" | "warn" | "error"`); exit code is `0` iff no check
//!   has status `"error"`.
