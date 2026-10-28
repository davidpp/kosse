//! Fails when the committed TypeScript bindings no longer match the Rust contracts.
//! Set `STACKSCAN_UPDATE_CONTRACTS=1` (see `bun run contracts`) to rewrite them instead.

use std::{env, fs, path::PathBuf};

use stackscan_core::contract::typescript_bindings;

const BINDINGS: &str = "web/src/generated/contracts.ts";

fn bindings_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(BINDINGS)
}

#[test]
fn typescript_bindings_are_current() {
    let generated = typescript_bindings();
    let path = bindings_path();

    if env::var("STACKSCAN_UPDATE_CONTRACTS").as_deref() == Ok("1") {
        fs::write(&path, &generated).expect("write generated bindings");
        return;
    }

    let committed = fs::read_to_string(&path).unwrap_or_default();
    assert!(
        committed == generated,
        "{BINDINGS} is stale; run `bun run contracts` and commit the result.\n\n--- committed\n{committed}\n--- generated\n{generated}"
    );
}
