//! Use-case orchestration. Composes pure `domain` types into the
//! "what should `runex doctor` actually check?" / "what should the
//! init handler write?" decisions, but stops short of the actual
//! file-system / registry / process-spawn calls — those live in
//! `infra`.
//!
//! Phase C scaffold. Modules from `runex-core` will land here in
//! the same commit that performs the workspace single-crate switch.
//! Today this directory only carries the rationale; the next commit
//! adds `config` (the parse/validate half), `doctor`, `init`,
//! `precache`, and `shell_export`.
//!
//! ## Dependency direction
//!
//! `app → domain` and `app → infra` (through trait boundaries
//! defined in `infra`). `app` must not import from `cmd` or `util`
//! because those live one layer further out and would create a
//! cycle.
