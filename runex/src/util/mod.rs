//! Leaf utility modules used across the runex CLI.
//!
//! Each submodule here is a *leaf*: it depends only on `runex_core`
//! and the standard library, and on platform crates where the helper
//! is platform-specific (`libc` for `O_NOFOLLOW`, `winreg`/`win_path`
//! for the augmented-PATH probe).
//!
//! What does *not* belong here:
//! * Anything with command-specific policy (the
//!   `install_rcfile_integration` family, `build_doctor_env_info`,
//!   `validate_bin`). Those ride along with their owning `cmd/*.rs`
//!   in Phase B Step B5.
//! * Runtime assembly (`AppContext`). Stays at crate root because it
//!   straddles util + handler concerns.
//!
//! Phase C will fold this directory into `runex/src/infra/` once
//! `runex-core` is absorbed into the binary crate; nothing here
//! changes shape, only its location.

pub mod path;
pub mod prompt;
pub mod shell;
