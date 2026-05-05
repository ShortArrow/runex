//! File / registry / env access. Everything that can fail because
//! the OS said no, or that needs platform-specific glue, lives here.
//!
//! Phase C scaffold. Modules from `runex-core` and from
//! `runex/src/win_path.rs` will land here in the same commit that
//! performs the workspace single-crate switch. Today this directory
//! only carries the rationale; the next commit adds `config_io` (the
//! file-handling half of the old `config.rs`), `integration_check`,
//! `env` (the `HomeDirResolver` trait + adapters), and `win_path`.
//!
//! ## Dependency direction
//!
//! `infra → domain` only. `infra` must not import from `app`, `cmd`,
//! or `util`. The trait definitions that `app` consumes for
//! injection (`HomeDirResolver`, future `ConfigStore` etc.) live
//! here too, so `app` has a single import target for "the outside
//! world".
