//! Pure logic. No I/O, no env reads, no process spawn. Every function
//! in this layer is `pub(crate)`-or-`pub` and deterministic given its
//! arguments. The whole subtree is testable without a filesystem
//! tempdir or env mutation.
//!
//! Phase C scaffold. Modules from `runex-core` will land here in
//! the same commit that performs the workspace single-crate switch.
//! Today this directory only carries the rationale; the next commit
//! adds `model`, `expand`, `hook`, `sanitize`, `timings`, and the
//! quoting half of `shell` (renamed `shell_quoting`).
//!
//! ## Dependency direction
//!
//! `domain` is the bottom of the stack. It must not import from
//! `app`, `infra`, `cmd`, or `util`. Any helper that needs file I/O
//! belongs in `infra`; any helper that orchestrates multiple `domain`
//! pieces (e.g. parse-then-validate) belongs in `app`.
