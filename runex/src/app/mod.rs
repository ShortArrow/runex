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
//! `app::config` holds parse + validate; the file I/O lives in
//! `infra::config_store` (Phase D D3b). The architecture rule
//! `no_filesystem_calls_in_app_layer` prevents `std::fs::*` from
//! drifting back into this directory.
//!
//! Use-case wrappers (`expand`, `hook`) re-export the operations
//! their cmd handlers need, so `cmd::expand` / `cmd::hook` /
//! `cmd::which` etc. don't import `crate::domain::*` for behaviour
//! — they go through `app::*`. The architecture rule
//! `no_cmd_to_domain_behavior_imports` enforces this.

pub mod config;
pub mod doctor;
pub mod expand;
pub mod hook;
pub mod init;
pub mod precache;
pub mod shell_export;
