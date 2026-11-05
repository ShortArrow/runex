//! Utility modules used across the runex CLI.
//!
//! The submodules depend on the standard library, `domain` and
//! `infra` (`infra::win_path` for the augmented-PATH probe on
//! Windows). They do not depend on `app`; `path` takes the
//! command-existence cache as an argument.
//!
//! What does *not* belong here:
//! * Anything with command-specific policy (the
//!   `install_rcfile_integration` family, `build_doctor_env_info`,
//!   `validate_bin`). Those ride along with their owning `cmd/*.rs`.
//! * Runtime assembly (`AppContext`). Stays at crate root because it
//!   straddles util + handler concerns.
//!
//! `util/` is a sibling of `cmd/`, not below `infra/`. The split
//! exists so `cmd/*` can reach these helpers without going
//! through the use-case layer (`app/*`); calls that need orchestration
//! must still go through `app/`.

pub mod path;
pub mod prompt;
pub mod shell;
