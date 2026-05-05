//! Use-case orchestration. Composes pure `domain` types into the
//! "what should `runex doctor` actually check?" / "what should the
//! init handler write?" decisions, but stops short of the actual
//! file-system / registry / process-spawn calls — those live in
//! `infra`.
//!
//! ## Dependency direction
//!
//! `app → domain` and `app → infra` (through trait boundaries
//! defined in `infra`). `app` must not import from `cmd` or `util`
//! because those live one layer further out and would create a
//! cycle.
//!
//! `config` currently mixes parse/validate (app concern) with file
//! I/O (infra concern); the file-handling half is intended to move
//! to `infra/config_io.rs` in a follow-up so this commit stays
//! move-only.

pub mod config;
pub mod doctor;
pub mod init;
pub mod precache;
