//! Embeds `web/dist` (produced by `bun run build`). Debug builds tolerate a missing bundle so Rust
//! tooling works on a fresh checkout; release builds refuse to ship a binary without its UI.
//!
//! Debug builds also get `KOSSE_PREVIEW_SUFFIX` (` (<branch>)`, or empty), the checkout's branch
//! for the preview window title, so a build an agent runs from a branch says which one. It reads
//! only the checkout's `HEAD` file, at build time; the binary never looks at this checkout.

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::ExitCode,
};

fn main() -> ExitCode {
    let dist = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/dist");
    // A directory path makes cargo rescan the whole bundle, so renamed hashed assets re-embed.
    println!("cargo::rerun-if-changed={}", dist.display());

    let release = env::var("PROFILE").is_ok_and(|profile| profile == "release");
    if !release {
        let checkout = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let branch = head_file(&checkout).and_then(|head| {
            println!("cargo::rerun-if-changed={}", head.display());
            let head = fs::read_to_string(head).ok()?;
            head.trim()
                .strip_prefix("ref: refs/heads/")
                .map(str::to_owned)
        });
        let suffix = branch.map_or_else(String::new, |branch| format!(" ({branch})"));
        println!("cargo::rustc-env=KOSSE_PREVIEW_SUFFIX={suffix}");
    }
    if release && !dist.join("index.html").is_file() {
        println!(
            "cargo::error=web/dist/index.html is missing; run `bun run build` so the release binary embeds the UI"
        );
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

/// `.git/HEAD`, or a linked worktree's own `HEAD` (`.git` is then a `gitdir: <path>` file).
fn head_file(checkout: &Path) -> Option<PathBuf> {
    let git = checkout.join(".git");
    // Only `HEAD` (and a worktree's `.git` pointer): watching the whole directory would rebuild
    // the app after every git operation.
    if git.is_dir() {
        return Some(git.join("HEAD"));
    }
    println!("cargo::rerun-if-changed={}", git.display());
    let pointer = fs::read_to_string(&git).ok()?;
    let dir = PathBuf::from(pointer.trim().strip_prefix("gitdir: ")?);
    Some(
        if dir.is_absolute() {
            dir
        } else {
            checkout.join(dir)
        }
        .join("HEAD"),
    )
}
