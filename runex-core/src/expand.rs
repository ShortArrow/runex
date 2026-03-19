use crate::model::{Config, ExpandResult};

/// Expand a token using the config.
///
/// `command_exists` is injected for testability (DI).
pub fn expand<F>(config: &Config, token: &str, command_exists: F) -> ExpandResult
where
    F: Fn(&str) -> bool,
{
    for abbr in &config.abbr {
        if abbr.key != token {
            continue;
        }
        // Infinite-loop guard: key == expand means no-op.
        if abbr.key == abbr.expand {
            continue;
        }
        // Check when_command_exists condition.
        if let Some(cmds) = &abbr.when_command_exists {
            if !cmds.iter().all(|c| command_exists(c)) {
                continue;
            }
        }
        return ExpandResult::Expanded(abbr.expand.clone());
    }
    ExpandResult::PassThrough(token.to_string())
}

/// List all abbreviations as (key, expand) pairs.
pub fn list(config: &Config) -> Vec<(&str, &str)> {
    config
        .abbr
        .iter()
        .map(|a| (a.key.as_str(), a.expand.as_str()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Abbr, Config};

    fn cfg(abbrs: Vec<Abbr>) -> Config {
        Config {
            version: 1,
            abbr: abbrs,
        }
    }

    fn abbr(key: &str, expand: &str) -> Abbr {
        Abbr {
            key: key.into(),
            expand: expand.into(),
            when_command_exists: None,
        }
    }

    fn abbr_when(key: &str, exp: &str, cmds: Vec<&str>) -> Abbr {
        Abbr {
            key: key.into(),
            expand: exp.into(),
            when_command_exists: Some(cmds.into_iter().map(String::from).collect()),
        }
    }

    #[test]
    fn match_expands() {
        let c = cfg(vec![abbr("gcm", "git commit -m")]);
        assert_eq!(
            expand(&c, "gcm", |_| true),
            ExpandResult::Expanded("git commit -m".into())
        );
    }

    #[test]
    fn no_match_passes_through() {
        let c = cfg(vec![abbr("gcm", "git commit -m")]);
        assert_eq!(
            expand(&c, "xyz", |_| true),
            ExpandResult::PassThrough("xyz".into())
        );
    }

    #[test]
    fn selects_correct_abbr() {
        let c = cfg(vec![
            abbr("gcm", "git commit -m"),
            abbr("gp", "git push"),
        ]);
        assert_eq!(
            expand(&c, "gp", |_| true),
            ExpandResult::Expanded("git push".into())
        );
    }

    #[test]
    fn key_eq_expand_passes_through() {
        let c = cfg(vec![abbr("ls", "ls")]);
        assert_eq!(
            expand(&c, "ls", |_| true),
            ExpandResult::PassThrough("ls".into())
        );
    }

    #[test]
    fn when_command_exists_present() {
        let c = cfg(vec![abbr_when("ls", "lsd", vec!["lsd"])]);
        assert_eq!(
            expand(&c, "ls", |_| true),
            ExpandResult::Expanded("lsd".into())
        );
    }

    #[test]
    fn when_command_exists_absent() {
        let c = cfg(vec![abbr_when("ls", "lsd", vec!["lsd"])]);
        assert_eq!(
            expand(&c, "ls", |_| false),
            ExpandResult::PassThrough("ls".into())
        );
    }

    #[test]
    fn list_returns_all_pairs() {
        let c = cfg(vec![
            abbr("gcm", "git commit -m"),
            abbr("gp", "git push"),
        ]);
        let pairs = list(&c);
        assert_eq!(pairs, vec![("gcm", "git commit -m"), ("gp", "git push")]);
    }
}
