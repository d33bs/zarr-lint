//! Build script that embeds optional development build metadata.
//!
//! The semantic package version always comes from Cargo (`CARGO_PKG_VERSION`).
//! Here we additionally capture the short git commit so that
//! `zarr-lint version --verbose` can surface it. When git is unavailable (for
//! example building from a source tarball), the commit is reported as
//! `unknown`; it never replaces or mutates the package version.

use std::process::Command;

fn main() {
    let commit = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string());

    println!("cargo:rustc-env=ZARRLINT_GIT_COMMIT={commit}");

    // Refresh the embedded commit when HEAD moves. The path is relative to this
    // crate's directory; if it is absent (no git checkout) cargo simply ignores
    // it.
    println!("cargo:rerun-if-changed=../../.git/HEAD");
}
