use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=RUNEX_GIT_COMMIT");

    // Re-run when the working tree's HEAD moves so the embedded commit hash
    // tracks `git rev-parse HEAD`. Without these, cargo only re-runs build.rs
    // when build.rs itself changes, leaving stale hashes after `git checkout`
    // / `git commit`.
    register_git_head_watch();

    if let Ok(commit) = std::env::var("RUNEX_GIT_COMMIT") {
        let commit = commit.trim();
        if !commit.is_empty() {
            println!("cargo:rustc-env=RUNEX_GIT_COMMIT={commit}");
            return;
        }
    }

    if let Ok(output) = Command::new("git")
        .args(["rev-parse", "--short=12", "HEAD"])
        .output()
    {
        if output.status.success() {
            let commit = String::from_utf8_lossy(&output.stdout);
            let commit = commit.trim();
            if !commit.is_empty() {
                println!("cargo:rustc-env=RUNEX_GIT_COMMIT={commit}");
            }
        }
    }
}

/// Tell cargo to invalidate the build whenever the current branch's HEAD
/// moves. We watch `.git/HEAD` (which itself changes on branch switches) and
/// the file the symbolic ref points at (which changes on commits).
fn register_git_head_watch() {
    let Ok(output) = Command::new("git")
        .args(["rev-parse", "--git-dir"])
        .output()
    else {
        return;
    };
    if !output.status.success() {
        return;
    }
    let git_dir_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if git_dir_str.is_empty() {
        return;
    }
    let git_dir = PathBuf::from(git_dir_str);

    let head_path = git_dir.join("HEAD");
    if head_path.exists() {
        println!("cargo:rerun-if-changed={}", head_path.display());
    }

    // If HEAD is "ref: refs/heads/<branch>", watch that file too — that's
    // what changes on `git commit` while staying on the same branch.
    if let Ok(head_contents) = std::fs::read_to_string(&head_path) {
        if let Some(ref_path) = head_contents.trim().strip_prefix("ref: ") {
            let target = git_dir.join(ref_path);
            if target.exists() {
                println!("cargo:rerun-if-changed={}", target.display());
            }
        }
    }
}
