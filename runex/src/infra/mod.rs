//! File / registry / env access. Everything that can fail because
//! the OS said no, or that needs platform-specific glue, lives here.
//!
//! ## Dependency direction
//!
//! `infra → domain` only. `infra` must not import from `app`, `cmd`,
//! or `util`. The trait definitions that `app` consumes for
//! injection (`HomeDirResolver`) live
//! here too, so `app` has a single import target for "the outside
//! world".

pub mod clipboard;
pub mod config_store;
pub mod env;
pub mod integration_cache;
pub mod integration_check;
pub mod shell_alias;
#[cfg(windows)]
pub mod win_path;
