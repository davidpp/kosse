//! The JSON Schema `kosse config schema` emits: committed, current, and accepting the example the
//! blueprint ships (which the Rust validator must accept too). Set `KOSSE_UPDATE_CONTRACTS=1`
//! (`bun run contracts`) to rewrite the committed copy.
#![expect(
    clippy::expect_used,
    reason = "test helpers fail the test by panicking"
)]

use std::{env, fs, path::PathBuf};

use kosse_core::profiles::{json_schema, source::parse_profile};

const SCHEMA: &str = "docs/kosse.schema.json";
const EXAMPLE_SOURCE: &str = "blueprint/project-metadata.md";

fn repo_file(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

/// The first fenced JSON block of the metadata blueprint.
fn shipped_example() -> String {
    let text = fs::read_to_string(repo_file(EXAMPLE_SOURCE)).expect("read blueprint");
    let start = text.find("```json\n").expect("a json block") + "```json\n".len();
    let end = start + text[start..].find("```").expect("closed block");
    text[start..end].to_owned()
}

#[test]
fn committed_schema_is_current() {
    let generated = json_schema();
    let path = repo_file(SCHEMA);
    if env::var("KOSSE_UPDATE_CONTRACTS").as_deref() == Ok("1") {
        fs::write(&path, &generated).expect("write schema");
        return;
    }
    let committed = fs::read_to_string(&path).unwrap_or_default();
    assert!(
        committed == generated,
        "{SCHEMA} is stale; run `bun run contracts` and commit the result."
    );
}

#[test]
fn schema_and_validator_accept_the_shipped_example() {
    let schema: serde_json::Value = serde_json::from_str(&json_schema()).expect("schema JSON");
    let validator = jsonschema::validator_for(&schema).expect("a valid JSON Schema");
    let example = shipped_example();
    let instance: serde_json::Value = serde_json::from_str(&example).expect("example JSON");
    let errors: Vec<String> = validator
        .iter_errors(&instance)
        .map(|error| error.to_string())
        .collect();
    assert!(errors.is_empty(), "{errors:?}");
    let profile = parse_profile(example.as_bytes()).expect("the validator accepts it too");
    assert_eq!(profile.services.len(), 2);
}

#[test]
fn schema_refuses_what_the_validator_refuses() {
    let schema: serde_json::Value = serde_json::from_str(&json_schema()).expect("schema JSON");
    let validator = jsonschema::validator_for(&schema).expect("a valid JSON Schema");
    for bad in [
        r#"{"version": 2}"#,
        r#"{"version": 1, "services": {"-x": {}}}"#,
        r#"{"version": 1, "services": {"web": {"links": [{"label": "x", "url": "javascript:alert(1)"}]}}}"#,
        r#"{"version": 1, "services": {"web": {"probePath": "//evil.example/"}}}"#,
        r#"{"version": 1, "services": {"web": {"match": {"cwd": "a", "command": "x"}}}}"#,
        r#"{"version": 1, "services": {"web": {"protocol": "gopher"}}}"#,
    ] {
        let instance: serde_json::Value = serde_json::from_str(bad).expect("JSON");
        assert!(!validator.is_valid(&instance), "schema accepted {bad}");
        assert!(
            parse_profile(bad.as_bytes()).is_err(),
            "validator accepted {bad}"
        );
    }
}
