//! Internal library crate for the runex CLI.
//!
//! # Not a public API
//!
//! `runex-core` is published to crates.io only because the `runex`
//! binary depends on it via a version constraint, which `cargo
//! publish` requires. **External code should not depend on
//! `runex-core` directly.** Items in this crate are renamed,
//! restructured, or removed without notice across patch releases.
//!
//! Semver guarantees apply only to the `runex` CLI's user-facing
//! surface:
//!
//! - The `config.toml` schema (documented in
//!   `docs/config-reference.md`).
//! - The `runex hook` per-keystroke RPC output format (line / cursor
//!   directives the shell wrappers eval).
//! - The `runex doctor --json` output: a top-level JSON array of
//!   check objects, each with at least `name` (string) and `status`
//!   (one of `"ok" | "warn" | "error"`); exit code is `0` iff no
//!   check has status `"error"`.
//!
//! If you have a use case that would benefit from a stable lib
//! interface, please open an issue at
//! <https://github.com/ShortArrow/runex/issues> rather than depending
//! on `runex-core` directly.
//!
//! # Future direction
//!
//! `runex-core` is scheduled for absorption into the `runex` crate
//! itself in a future patch release. After that lands, `runex-core
//! 0.1.x` will remain on crates.io for any cargo lockfile that
//! references it but will receive no further updates. Internal
//! reorganisation will continue inside the `runex` crate as a
//! `domain` / `app` / `infra` module split.
//!
//! # Module layout (current, internal)
//!
//! - [`model`] — pure data types (`Config`, `Abbr`, `Shell`, …).
//! - [`expand`] — token-to-cast expansion logic (pure).
//! - [`hook`] — per-keystroke decision logic (pure).
//! - [`sanitize`] — character/string classification (pure).
//! - [`shell`] — shell-specific quoting and integration script generation.
//! - [`timings`] — timing record types for `runex timings`.
//! - [`config`] — TOML parse/validate + file I/O.
//! - [`env`] — `HomeDirResolver` trait and adapters for tests.
//! - [`init`] — `runex init` helpers (config seed, integration line).
//! - [`doctor`] — environment health checks.
//! - [`integration_check`] — rcfile-marker / clink-lua probes.
//! - [`precache`] — deprecated; the `[precache]` section is a no-op
//!   since 0.1.12.

pub mod config;
pub mod doctor;
pub mod env;
pub mod expand;
pub mod hook;
pub mod init;
pub mod integration_check;
pub mod model;
pub mod precache;
pub mod sanitize;
pub mod shell;
pub mod timings;
