//! `runex version` — print the version string (and optionally git commit).

use crate::{format::version_line, CmdOutcome, CmdResult, GIT_COMMIT};

pub fn handle(json: bool) -> CmdResult {
    if json {
        #[derive(serde::Serialize)]
        struct VersionJson<'a> {
            version: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            commit: Option<&'a str>,
        }
        let v = VersionJson {
            version: env!("CARGO_PKG_VERSION"),
            commit: GIT_COMMIT.filter(|s| !s.is_empty()),
        };
        println!("{}", serde_json::to_string_pretty(&v)?);
    } else {
        println!("{}", version_line());
    }
    Ok(CmdOutcome::Ok)
}
