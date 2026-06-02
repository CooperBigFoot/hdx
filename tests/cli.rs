//! Smoke tests for the `hdx` bin: confirm each subcommand parses a path, calls
//! its `hdx-core` verb, and emits a JSON object on stdout. Exit-code semantics
//! are S2; here we only assert the surface emits JSON.

use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

/// Resolve a fixture dir relative to the bin crate root (the workspace root).
fn fixture(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel)
}

/// Run the built `hdx` bin with the given args and return captured stdout bytes,
/// asserting the process did not fail to launch.
fn run_hdx(args: &[&str]) -> Vec<u8> {
    let output = Command::new(env!("CARGO_BIN_EXE_hdx"))
        .args(args)
        .output()
        .expect("failed to launch hdx binary");
    output.stdout
}

#[test]
fn describe_emits_json_object_to_stdout() {
    let path = fixture("conformance/valid/minimal");
    let stdout = run_hdx(&["describe", path.to_str().expect("fixture path is valid UTF-8")]);

    assert!(!stdout.is_empty(), "describe produced empty stdout");

    let value: Value = serde_json::from_slice(&stdout).expect("describe stdout is not valid JSON");
    assert!(value.is_object(), "describe stdout is not a JSON object");
}

#[test]
fn validate_emits_report_object_with_conformant_key() {
    let path = fixture("conformance/valid/minimal");
    let stdout = run_hdx(&["validate", path.to_str().expect("fixture path is valid UTF-8")]);

    assert!(!stdout.is_empty(), "validate produced empty stdout");

    let value: Value = serde_json::from_slice(&stdout).expect("validate stdout is not valid JSON");
    let object = value
        .as_object()
        .expect("validate stdout is not a JSON object");
    assert!(
        object.contains_key("conformant"),
        "validate report is missing the `conformant` key"
    );
}
