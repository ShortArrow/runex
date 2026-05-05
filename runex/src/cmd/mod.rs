//! Per-subcommand handler modules.
//!
//! Each `cmd::<name>` module owns:
//! * its `handle_<name>` entry point (called from `main()`'s
//!   dispatch),
//! * any helpers / constants that exist *only* to serve that
//!   handler (validation logic, atomic-write policy, doctor env-
//!   info builders).
//!
//! What lives at the crate root instead:
//! * `Cli` / `Commands` (clap derive) — CLI surface, not handler
//!   internals.
//! * `AppContext` / `OptionalContext` — runtime assembly used by
//!   multiple handlers.
//! * `CmdOutcome` / `CmdResult` — the contract every handler
//!   returns through.
//! * `Spinner`, `resolve_config`, `compute_precache_fingerprint` —
//!   pre-handler utilities.
//!
//! What lives in `util/` instead: leaf helpers that have *no*
//! command-specific policy (shell detection, the
//! `command_exists` factory, the y/N prompt). See `util/mod.rs`.
//!
//! ## Layering
//!
//! `cmd → app → {domain, infra}`. Handlers must not import
//! `crate::domain::expand` / `crate::domain::hook` / orchestration
//! symbols from `domain::shell` directly — go through `app::*`
//! use-case wrappers instead. The
//! `runex/tests/architecture.rs::no_cmd_to_domain_behavior_imports`
//! test pins the contract.

pub mod add_remove;
pub mod doctor;
pub mod expand;
pub mod export;
pub mod hook;
pub mod init;
pub mod list;
pub mod precache;
pub mod timings;
pub mod version;
pub mod which;
