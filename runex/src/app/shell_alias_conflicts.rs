//! Report abbreviation keys that collide with a shell alias, for
//! `runex doctor`.
//!
//! The aliases come from [`crate::infra::shell_alias`]; this module
//! turns a collision into a doctor [`Check`].

use crate::app::doctor::{Check, CheckStatus, DiagResult};
use crate::domain::model::{Abbr, Config};
use crate::domain::sanitize::sanitize_for_display;
use crate::infra::shell_alias::{load_bash_aliases, load_pwsh_aliases};

pub(crate) fn check_pwsh_alias_with<F>(token: &str, lookup: F) -> Option<Check>
where
    F: Fn(&str) -> Option<String>,
{
    let definition = lookup(token)?;
    Some(Check {
        name: format!("shell:pwsh:key:{}", sanitize_for_display(token)),
        status: CheckStatus::Warn,
        detail: format!(
            "conflicts with existing alias '{}' -> {}",
            sanitize_for_display(token),
            sanitize_for_display(&definition)
        ),
        detail_verbose: None,
    })
}

pub(crate) fn check_bash_alias_with<F>(token: &str, lookup: F) -> Option<Check>
where
    F: Fn(&str) -> Option<String>,
{
    let detail = lookup(token)?;
    Some(Check {
        name: format!("shell:bash:key:{}", sanitize_for_display(token)),
        status: CheckStatus::Warn,
        detail: format!("conflicts with existing alias {}", sanitize_for_display(&detail)),
        detail_verbose: None,
    })
}

pub(crate) fn collect_shell_alias_conflicts_with<FPwsh, FBash>(
    abbrs: &[Abbr],
    pwsh_lookup: FPwsh,
    bash_lookup: FBash,
) -> Vec<Check>
where
    FPwsh: Fn(&str) -> Option<String> + Copy,
    FBash: Fn(&str) -> Option<String> + Copy,
{
    let mut checks = Vec::new();
    for abbr in abbrs {
        if let Some(check) = check_pwsh_alias_with(&abbr.key, pwsh_lookup) {
            checks.push(check);
        }
        if let Some(check) = check_bash_alias_with(&abbr.key, bash_lookup) {
            checks.push(check);
        }
    }
    checks
}

pub(crate) fn add_shell_alias_conflicts(result: &mut DiagResult, config: Option<&Config>) {
    let Some(config) = config else {
        return;
    };
    let pwsh_aliases = load_pwsh_aliases();
    let bash_aliases = load_bash_aliases();

    result
        .checks
        .extend(collect_shell_alias_conflicts_with(
            &config.abbr,
            |token| pwsh_aliases.get(token).cloned(),
            |token| bash_aliases.get(token).cloned(),
        ));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_abbr(key: &str) -> Abbr {
        Abbr {
            key: key.into(),
            expand: crate::domain::model::PerShellString::All(format!("expand-{key}")),
            when_command_exists: None,
            number: None,
            match_kind: None,
        }
    }

    #[test]
    fn collect_shell_alias_conflicts_reports_pwsh_and_bash() {
        let checks = collect_shell_alias_conflicts_with(
            &[test_abbr("gcm"), test_abbr("nv")],
            |token| (token == "gcm").then_some("Get-Command".to_string()),
            |token| (token == "nv").then_some("alias nv='nvim'".to_string()),
        );

        assert_eq!(checks.len(), 2);
        assert_eq!(checks[0].name, "shell:pwsh:key:gcm");
        assert!(checks[0].detail.contains("Get-Command"));
        assert_eq!(checks[1].name, "shell:bash:key:nv");
        assert!(checks[1].detail.contains("alias nv='nvim'"));
    }

    #[test]
    fn collect_shell_alias_conflicts_skips_missing_aliases() {
        let checks = collect_shell_alias_conflicts_with(&[test_abbr("gcm")], |_| None, |_| None);
        assert!(checks.is_empty());
    }

    #[test]
    fn check_pwsh_alias_name_strips_control_chars_from_key() {
        let checks = collect_shell_alias_conflicts_with(
            &[test_abbr("key\x1b[2Jevil")],
            |_token| Some("Get-Command".to_string()),
            |_token| None,
        );
        assert_eq!(checks.len(), 1);
        assert!(
            !checks[0].name.contains('\x1b'),
            "shell:pwsh check name must not contain raw ESC: {:?}", checks[0].name
        );
    }

    #[test]
    fn check_bash_alias_name_strips_control_chars_from_key() {
        let checks = collect_shell_alias_conflicts_with(
            &[test_abbr("key\x1b[2Jevil")],
            |_token| None,
            |_token| Some("alias key='evil'".to_string()),
        );
        assert_eq!(checks.len(), 1);
        assert!(
            !checks[0].name.contains('\x1b'),
            "shell:bash check name must not contain raw ESC: {:?}", checks[0].name
        );
    }

    #[test]
    fn check_pwsh_alias_detail_strips_control_chars_from_definition() {
        let checks = collect_shell_alias_conflicts_with(
            &[test_abbr("gcm")],
            |_token| Some("Get-Command\x1b[31mRED\x1b[0m".to_string()),
            |_token| None,
        );
        assert_eq!(checks.len(), 1);
        assert!(
            !checks[0].detail.contains('\x1b'),
            "shell:pwsh check detail must not contain raw ESC from definition: {:?}", checks[0].detail
        );
    }
}
