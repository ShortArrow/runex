//! File / registry / env access. Everything that can fail because
//! the OS said no, or that needs platform-specific glue, lives here.
//!
//! ## Dependency direction
//!
//! `infra → domain` only. `infra` must not import from `app`, `cmd`,
//! or `util`. The trait definitions that `app` consumes for
//! injection (`HomeDirResolver`, future `ConfigStore` etc.) live
//! here too, so `app` has a single import target for "the outside
//! world".
//!
//! `win_path` (the Windows process+HKCU+HKLM PATH augmentation)
//! still lives at `runex/src/win_path.rs` because it predates this
//! split; moving it here is a separate commit to keep this one
//! move-only across the runex-core absorption.

pub mod clipboard;
pub mod config_store;
pub mod env;
pub mod integration_cache;
pub mod integration_check;
