//! `runex-core` 0.1.x — **deprecated, unused, scheduled for deletion.**
//!
//! Every module that used to live here moved to `runex/src/{domain,
//! app, infra}/` in the Phase C absorption. The `runex` binary no
//! longer depends on this crate. The crate stays in the workspace
//! for one more commit so the file-move history is reviewable as a
//! single `git mv`-shaped diff; the very next commit removes it.
//!
//! If you stumble on this on crates.io: do nothing — `cargo install
//! runex` does not require `runex-core 0.1.x` and never re-publishes
//! it. If your own project depends on `runex-core` directly, please
//! migrate to `runex` as a library or open an issue at
//! <https://github.com/ShortArrow/runex/issues>.
