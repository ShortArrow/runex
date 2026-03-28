use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=RUNEX_GIT_COMMIT");

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
