//! Leaf utility modules used across the runex CLI.
//!
//! Each submodule here is a *leaf*: it depends only on the standard
//! library, the in-crate `domain`/`infra` types, and platform
//! crates where the helper is platform-specific (`libc` for
//! `O_NOFOLLOW`, `winreg`/`win_path` for the augmented-PATH probe).
//!
//! What does *not* belong here:
//! * Anything with command-specific policy (the
//!   `install_rcfile_integration` family, `build_doctor_env_info`,
//!   `validate_bin`). Those ride along with their owning `cmd/*.rs`.
//! * Runtime assembly (`AppContext`). Stays at crate root because it
//!   straddles util + handler concerns.
//!
//! `util/` is a sibling of `cmd/`, not below `infra/`. The split
//! exists so `cmd/*` can reach truly leaf helpers without going
//! through the use-case layer (`app/*`); calls that need orchestration
//! must still go through `app/`.

pub mod path;
pub mod prompt;
pub mod shell;
