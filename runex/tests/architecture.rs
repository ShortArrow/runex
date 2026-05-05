//! Compile-time-ish architecture rules for the `runex` crate.
//!
//! Walks `runex/src/` from inside the cargo test runner and asserts
//! the layering contract: `cmd → app → {domain, infra}`,
//! `infra → domain`, nothing imports upward, no cycles. The rules
//! exist so a future refactor can't quietly re-introduce the
//! `app::doctor → infra::integration_check → app::init` cycle that
//! Phase D removed (or any other circular import).
//!
//! ## Why a test instead of `cargo deny`
//!
//! The single-process test runs in <30 ms, has zero external
//! dependencies, and shows up in the same green-bar / red-bar gate
//! as the rest of the suite. `cargo deny` would need a separate
//! config file and a CI step; for a 12-file `src/` tree the cost is
//! much higher than the value.
//!
//! ## How rules are expressed
//!
//! Each rule is a triple of `(layer_we_check, forbidden_use_prefix,
//! human-readable rationale)`. The walker only inspects `use`
//! lines — fully-qualified paths inside function bodies (rare in
//! this crate, and a code smell in their own right) are out of
//! scope. If a violation slips in via `crate::foo::bar()` instead
//! of `use crate::foo`, the next refactor will surface it.

use std::fs;
use std::path::{Path, PathBuf};

/// Return every non-comment, non-`use`, non-test-mod source line in
/// `content` together with its 1-indexed number. Mirrors `use_lines`'s
/// `#[cfg(test)] mod` skipping so layering rules apply only to the
/// production code that ships in the binary.
///
/// Comment-only lines and lines whose first non-whitespace token is
/// `//` are dropped. Trailing `// foo` comments stay in the line — the
/// substring rules below check for the *call site* (`std::fs::write(`)
/// which won't appear inside a comment by accident.
fn production_lines(content: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut depth: usize = 0;
    let mut test_mod_depths: Vec<usize> = Vec::new();
    let mut prev_line_marks_test_mod = false;
    for (i, raw_line) in content.lines().enumerate() {
        let line = raw_line;
        let trimmed = line.trim_start();
        let attr_marks_test = trimmed.starts_with("#[cfg(test)]")
            || trimmed.starts_with("#[cfg(any(test")
            || trimmed.starts_with("#[cfg(all(test");
        if test_mod_depths.is_empty() && !trimmed.starts_with("//") {
            out.push((i + 1, line.to_string()));
        }
        for (col, ch) in line.char_indices() {
            if ch == '/' && line.as_bytes().get(col + 1) == Some(&b'/') {
                break;
            }
            match ch {
                '{' => {
                    depth += 1;
                    if prev_line_marks_test_mod
                        && (trimmed.starts_with("mod ") || trimmed.contains(" mod "))
                    {
                        test_mod_depths.push(depth);
                        prev_line_marks_test_mod = false;
                    }
                }
                '}' => {
                    if let Some(&top) = test_mod_depths.last() {
                        if depth == top {
                            test_mod_depths.pop();
                        }
                    }
                    depth = depth.saturating_sub(1);
                }
                _ => {}
            }
        }
        if attr_marks_test {
            if trimmed.contains("mod ") && trimmed.contains('{') {
                test_mod_depths.push(depth);
            } else {
                prev_line_marks_test_mod = true;
            }
        } else if !attr_marks_test
            && !trimmed.is_empty()
            && !trimmed.starts_with("//")
            && !trimmed.starts_with("#[")
        {
            prev_line_marks_test_mod = false;
        }
    }
    out
}

/// Recursively collect every `.rs` file under `dir`, returning
/// `(repo-relative path, contents)` pairs.
fn collect_rs_files(dir: &Path) -> Vec<(PathBuf, String)> {
    let mut out = Vec::new();
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return out,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(collect_rs_files(&path));
        } else if path.extension().is_some_and(|e| e == "rs") {
            if let Ok(content) = fs::read_to_string(&path) {
                out.push((path, content));
            }
        }
    }
    out
}

/// Return every `use crate::…;` line in `content` outside any
/// `#[cfg(test)] mod …` block, together with its 1-indexed line
/// number.
///
/// Comments and string literals that happen to contain `use crate::`
/// are ignored — we only consider lines where the `use` is the first
/// non-whitespace token.
///
/// Test modules are skipped because layering rules apply to
/// production code; an inline `mod tests` may legitimately reach
/// across layers to wire up a fixture (e.g. `domain::hook` tests
/// importing `app::config::parse_config` to build a sample config).
/// The rule of thumb is "what gets compiled into the binary must
/// obey the layering"; test-only code is exempt.
fn use_lines(content: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut depth: usize = 0;            // depth of `{ }` we're nested in
    let mut test_mod_depths: Vec<usize> = Vec::new();
    let mut prev_line_marks_test_mod = false;
    for (i, raw_line) in content.lines().enumerate() {
        let line = raw_line;
        let trimmed = line.trim_start();

        // Detect `#[cfg(test)]` (possibly on its own line, possibly
        // followed by `pub` or attributes); we don't try to parse the
        // attribute fully — the `mod foo {` opener on the *next* line
        // is what we use to mark the depth.
        let attr_marks_test = trimmed.starts_with("#[cfg(test)]")
            || trimmed.starts_with("#[cfg(any(test")
            || trimmed.starts_with("#[cfg(all(test");

        // Walk braces *before* extracting the use line so the depth
        // accounting is correct for the current line.
        if !test_mod_depths.is_empty() {
            // Currently inside a test mod: track nested braces so we
            // exit at the right depth.
        }

        // Capture `use crate::…` lines that aren't inside a test mod.
        if trimmed.starts_with("use crate::") || trimmed.starts_with("pub use crate::") {
            if test_mod_depths.is_empty() {
                out.push((i + 1, line.to_string()));
            }
        }

        // Naive brace counter that ignores braces inside string /
        // char literals and `//` comments. Good enough for this
        // crate; if we hit a corner case the test failure will be
        // obvious.
        for (col, ch) in line.char_indices() {
            // Skip past `//` comment to end of line.
            if ch == '/' && line.as_bytes().get(col + 1) == Some(&b'/') {
                break;
            }
            match ch {
                '{' => {
                    depth += 1;
                    if prev_line_marks_test_mod
                        && (trimmed.starts_with("mod ") || trimmed.contains(" mod "))
                    {
                        test_mod_depths.push(depth);
                        prev_line_marks_test_mod = false;
                    }
                }
                '}' => {
                    if let Some(&top) = test_mod_depths.last() {
                        if depth == top {
                            test_mod_depths.pop();
                        }
                    }
                    depth = depth.saturating_sub(1);
                }
                _ => {}
            }
        }

        // Mark "next opening `mod` belongs to a test mod" when this
        // line is `#[cfg(test)]`. The brace that follows might be on
        // the same line (`#[cfg(test)] mod tests { … }`) or the next.
        if attr_marks_test {
            if trimmed.contains("mod ") && trimmed.contains('{') {
                test_mod_depths.push(depth); // already opened in same line
            } else {
                prev_line_marks_test_mod = true;
            }
        } else if !attr_marks_test
            && !trimmed.is_empty()
            && !trimmed.starts_with("//")
            && !trimmed.starts_with("#[")
        {
            // Any non-attribute, non-comment line resets the
            // "next mod is test" flag.
            prev_line_marks_test_mod = false;
        }
    }
    out
}

/// Crate root for the runex package, derived from `CARGO_MANIFEST_DIR`.
/// `runex/tests/architecture.rs` runs with the cargo manifest pointed
/// at `runex/Cargo.toml`, so `src/` is right next to us.
fn src_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// `infra` must not import from `app`. The Phase D split moved
/// `RUNEX_INIT_MARKER` and `rc_file_for*` out of `app::init` into
/// `infra` precisely so this rule can hold; if it ever fires again
/// we have a cycle (`app → infra → app`).
#[test]
fn no_infra_to_app_imports() {
    let infra_dir = src_root().join("infra");
    let mut violations = Vec::new();
    for (path, content) in collect_rs_files(&infra_dir) {
        for (lineno, line) in use_lines(&content) {
            if line.contains("crate::app") {
                violations.push(format!(
                    "{}:{lineno}: {}",
                    path.strip_prefix(src_root().parent().unwrap())
                        .unwrap_or(&path)
                        .display(),
                    line.trim()
                ));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "infra/ must not import from app/. Found:\n  {}",
        violations.join("\n  ")
    );
}

/// `domain` must not import from any sibling layer. Domain is the
/// bottom of the stack — any `use crate::{app,infra,cmd,util}` in
/// here is a hint that orchestration leaked into pure logic.
#[test]
fn no_domain_to_anyone_else_imports() {
    let domain_dir = src_root().join("domain");
    let forbidden = ["crate::app", "crate::infra", "crate::cmd", "crate::util"];
    let mut violations = Vec::new();
    for (path, content) in collect_rs_files(&domain_dir) {
        for (lineno, line) in use_lines(&content) {
            for needle in forbidden {
                if line.contains(needle) {
                    violations.push(format!(
                        "{}:{lineno}: {}",
                        path.strip_prefix(src_root().parent().unwrap())
                            .unwrap_or(&path)
                            .display(),
                        line.trim()
                    ));
                }
            }
        }
    }
    assert!(
        violations.is_empty(),
        "domain/ must not import from app/infra/cmd/util. Found:\n  {}",
        violations.join("\n  ")
    );
}

/// `cmd` must not import behavior modules of `domain` directly —
/// it should go through `app::*` use-cases instead. DTO imports
/// (`domain::model`, `domain::shell::Shell`, `domain::sanitize`)
/// stay legal because cmd needs the type names to pass values
/// around.
///
/// Forbidden today: `crate::domain::expand`, `crate::domain::hook`,
/// `crate::domain::shell::export_script` (the orchestration symbol
/// inside an otherwise pure module — Phase D moves it to
/// `app::shell_export`, after which a stricter rule can ban every
/// `crate::domain::shell::*` except the `Shell` type itself).
#[test]
fn no_cmd_to_domain_behavior_imports() {
    let cmd_dir = src_root().join("cmd");
    // `crate::domain::shell::export_script` shows up in `use crate::
    // domain::shell::export_script` style or as
    // `use crate::domain::shell::{export_script, Shell}`. The
    // simple substring catches both. Phase D D2 will remove the
    // need for this needle entirely — once `export_script` lives in
    // `app`, no `cmd/*` will reference it from `domain`.
    let forbidden = [
        "crate::domain::expand",
        "crate::domain::hook",
        // `export_script` is the only orchestration symbol that
        // historically lived in `domain::shell`. Phase D D2 moves it
        // to `app::shell_export`; this rule enforces that cmd
        // callers go through `app` from then on. The substring
        // catches both the explicit `use crate::domain::shell::
        // export_script` form and the bracket form
        // `use crate::domain::shell::{export_script, Shell}`.
        "crate::domain::shell::export_script",
    ];
    let mut violations = Vec::new();
    for (path, content) in collect_rs_files(&cmd_dir) {
        for (lineno, line) in use_lines(&content) {
            for needle in forbidden {
                if line.contains(needle) {
                    violations.push(format!(
                        "{}:{lineno}: {}",
                        path.strip_prefix(src_root().parent().unwrap())
                            .unwrap_or(&path)
                            .display(),
                        line.trim()
                    ));
                }
            }
        }
    }
    assert!(
        violations.is_empty(),
        "cmd/ must not import domain behavior modules (expand/hook). \
         Use the app/* usecase wrappers instead. Found:\n  {}",
        violations.join("\n  ")
    );
}

/// `app/` must stay file-system-free in production code: every
/// destructive `std::fs::*` call (`write`, `rename`, `remove_file`,
/// `OpenOptions`, `File::open`, `create_dir_all`, …) belongs in
/// `infra/`. Phase D D3 moves the config-file I/O from
/// `app/config.rs` into `infra/config_store.rs` precisely so this
/// rule can hold.
///
/// Only production lines are checked — `#[cfg(test)] mod` blocks are
/// allowed to touch the file system because the tests build their
/// own fixtures. The substring set is intentionally narrow to avoid
/// false positives on `std::fs::Metadata` (a type) or
/// `std::os::unix::fs::*` (a re-export trait).
#[test]
fn no_filesystem_calls_in_app_layer() {
    let app_dir = src_root().join("app");
    let needles = [
        "std::fs::write",
        "std::fs::read",
        "std::fs::rename",
        "std::fs::remove_file",
        "std::fs::create_dir",
        "std::fs::OpenOptions",
        "std::fs::File::open",
        "std::fs::File::create",
        "std::fs::canonicalize",
        "std::fs::symlink_metadata",
    ];
    let mut violations = Vec::new();
    for (path, content) in collect_rs_files(&app_dir) {
        for (lineno, line) in production_lines(&content) {
            for needle in needles {
                if line.contains(needle) {
                    violations.push(format!(
                        "{}:{lineno}: {}",
                        path.strip_prefix(src_root().parent().unwrap())
                            .unwrap_or(&path)
                            .display(),
                        line.trim()
                    ));
                }
            }
        }
    }
    assert!(
        violations.is_empty(),
        "app/ must not perform file-system calls in production code. \
         Move the I/O into infra/config_store.rs (or another infra/* \
         module). Found:\n  {}",
        violations.join("\n  ")
    );
}
