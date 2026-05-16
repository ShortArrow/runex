//! Use-case wrappers for the `expand` / `which` / `list` commands.
//!
//! Phase D D4a: every cmd handler used to call `crate::domain::expand`
//! directly, which made `cmd → domain` reach across the use-case
//! layer. This module re-publishes the same operations under the
//! `app::*` namespace so the architecture test
//! `no_cmd_to_domain_behavior_imports` can pin the contract: cmd
//! handlers go through `app/`, and DTO types
//! (`Config`, `ExpandResult`, `Shell`) cross the boundary as data.
//!
//! These wrappers add no logic on top of the domain functions —
//! they're a one-line indirection. Future versions of the use-case
//! could enrich them (e.g. adding metric hooks, tracing spans, or
//! caching), and call sites would not have to change.

use crate::domain::expand as domain_expand;
use crate::domain::model::{Config, ExpandResult};
use crate::domain::shell::Shell;
use crate::domain::timings::Timings;

pub(crate) use crate::domain::expand::WhichResult;

/// Compute the expansion for a single token. Untouched-on-skip
/// behaviour and self-loop guard live in `domain::expand`.
pub(crate) fn expand<F>(
    config: &Config,
    token: &str,
    shell: Shell,
    command_exists: F,
) -> ExpandResult
where
    F: Fn(&str) -> bool,
{
    domain_expand::expand(config, token, shell, command_exists)
}

/// Same as [`expand`], but threads a [`Timings`] collector through so
/// `runex timings` can break down per-phase cost.
pub(crate) fn expand_timed<F>(
    config: &Config,
    token: &str,
    shell: Shell,
    command_exists: F,
    timings: &mut Timings,
) -> ExpandResult
where
    F: Fn(&str) -> bool,
{
    domain_expand::expand_timed(config, token, shell, command_exists, timings)
}

/// Trace the rule-evaluation order for a token (powers `runex which`
/// and the `--dry-run` mode of `runex expand`). Returns every rule
/// considered, the matching rule (if any), and the reasons the
/// non-matching candidates were skipped.
pub(crate) fn which_abbr<F>(
    config: &Config,
    token: &str,
    shell: Shell,
    command_exists: F,
) -> WhichResult
where
    F: Fn(&str) -> bool,
{
    domain_expand::which_abbr(config, token, shell, command_exists)
}

/// `(key, expansion-for-this-shell)` pairs from the loaded config.
/// `runex list` prints the result; the use-case layer keeps the
/// borrow signature so callers don't allocate strings unnecessarily.
///
/// `filter` is an optional exact-key filter (issue #2): when `Some(key)`,
/// only the rule with that exact key is returned.
pub(crate) fn list_pairs<'a>(
    config: &'a Config,
    shell: Option<Shell>,
    filter: Option<&str>,
) -> Vec<(&'a str, String)> {
    domain_expand::list(config, shell, filter)
}
