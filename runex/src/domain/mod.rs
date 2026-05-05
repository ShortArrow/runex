//! Pure logic. No I/O, no env reads, no process spawn. Every function
//! in this layer is `pub(crate)`-or-`pub` and deterministic given its
//! arguments. The whole subtree is testable without a filesystem
//! tempdir or env mutation.
//!
//! ## Dependency direction
//!
//! `domain` is the bottom of the stack. It must not import from
//! `app`, `infra`, `cmd`, or `util`. Any helper that needs file I/O
//! belongs in `infra`; any helper that orchestrates multiple `domain`
//! pieces (e.g. parse-then-validate) belongs in `app`.
//!
//! `shell` currently includes the integration-script generator
//! (`export_script`) which has policy concerns that arguably belong
//! in `app`. Splitting `shell` into pure `shell_quoting` (here) and
//! `shell_export` (in `app`) is left for a follow-up so this commit
//! stays move-only.

pub mod expand;
pub mod hook;
pub mod model;
pub mod sanitize;
pub mod shell;
pub mod timings;
