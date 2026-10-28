//! Embeds `web/dist` (produced by `bun run build`). Debug builds tolerate a missing bundle so Rust
//! tooling works on a fresh checkout; release builds refuse to ship a binary without its UI.

use std::{env, path::Path, process::ExitCode};

fn main() -> ExitCode {
    let dist = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/dist");
    // A directory path makes cargo rescan the whole bundle, so renamed hashed assets re-embed.
    println!("cargo::rerun-if-changed={}", dist.display());

    let release = env::var("PROFILE").is_ok_and(|profile| profile == "release");
    if release && !dist.join("index.html").is_file() {
        println!(
            "cargo::error=web/dist/index.html is missing; run `bun run build` so the release binary embeds the UI"
        );
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
