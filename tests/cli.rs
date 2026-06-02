//! CLI integration tests for the `hdx` bin.
//!
//! Two layers, both invoking the built binary (`CARGO_BIN_EXE_hdx`) via
//! [`std::process::Command`] and inspecting the real process output:
//!
//! - **S1 smoke** — each subcommand parses a path, calls its `hdx-core` verb, and
//!   emits a JSON object on stdout.
//! - **S2 exit-code matrix** — the documented `0 / 1 / 2` exit-code contract, with
//!   the load-bearing distinction asserted against the committed fixtures: a
//!   `conformant: false` **report** (exit 1) vs a structural / entry **error**
//!   (exit 2, including the §0 hard cut, a nonexistent path, and a usage error).

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

/// Run the built `hdx` bin and return the captured `(exit_code, stdout)`.
///
/// The exit code is taken from `Output::status.code()`; a process terminated by a
/// signal (no code) is a launch-environment fault and panics the test.
fn run_hdx_full(args: &[&str]) -> (i32, Vec<u8>) {
    let output = Command::new(env!("CARGO_BIN_EXE_hdx"))
        .args(args)
        .output()
        .expect("failed to launch hdx binary");
    let code = output
        .status
        .code()
        .expect("hdx process was terminated by a signal (no exit code)");
    (code, output.stdout)
}

/// Convenience: the absolute fixture path as a `&str` arg.
fn fixture_arg(rel: &str) -> String {
    fixture(rel)
        .to_str()
        .expect("fixture path is valid UTF-8")
        .to_string()
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

// --- MS7-S2: the exit-code matrix (0 / 1 / 2), asserted against the fixtures -------
//
// The contract (spec §0/§10/§14, architecture §2):
//   0 = describe ok OR validate conformant:true
//   1 = validate conformant:false (a violated MUST that RAN — a report, not an error)
//   2 = structural / entry error (bad args, unreadable/nonexistent path, malformed
//       manifest, the §0 hard cut) — NEVER softened into a conformant:false report.

/// `describe` of the conformant fixture → exit 0, stdout a JSON object.
#[test]
fn describe_valid_minimal_exits_zero_with_json_object() {
    let (code, stdout) = run_hdx_full(&["describe", &fixture_arg("conformance/valid/minimal")]);

    assert_eq!(code, 0, "describe of a valid dataset must exit 0");
    let value: Value = serde_json::from_slice(&stdout).expect("describe stdout is valid JSON");
    assert!(value.is_object(), "describe stdout is a JSON object");
}

/// `validate` of the conformant fixture → exit 0, stdout `"conformant": true`.
#[test]
fn validate_valid_minimal_exits_zero_conformant_true() {
    let (code, stdout) = run_hdx_full(&["validate", &fixture_arg("conformance/valid/minimal")]);

    assert_eq!(code, 0, "validate of a conformant dataset must exit 0");
    let value: Value = serde_json::from_slice(&stdout).expect("validate stdout is valid JSON");
    assert_eq!(
        value.get("conformant").and_then(Value::as_bool),
        Some(true),
        "the conformant fixture reports conformant: true"
    );
}

/// `validate` of `invalid/missing-root-rollup` → exit 1, stdout `"conformant": false`.
///
/// The L1 failure is carried **in the report** (a violated `MUST` that ran), not as
/// an `Err` — this is the load-bearing exit-1 ≠ exit-2 distinction.
#[test]
fn validate_missing_root_rollup_exits_one_conformant_false_report() {
    let (code, stdout) = run_hdx_full(&[
        "validate",
        &fixture_arg("conformance/invalid/missing-root-rollup"),
    ]);

    assert_eq!(
        code, 1,
        "a non-conformant report must exit 1 (distinct from the exit-2 error path)"
    );
    let value: Value =
        serde_json::from_slice(&stdout).expect("validate stdout is valid JSON even when exit 1");
    assert_eq!(
        value.get("conformant").and_then(Value::as_bool),
        Some(false),
        "the L1 fail is carried as conformant: false in the report, not as an error"
    );
}

/// `validate` of `invalid/wrong-format-version` → exit 2 (the §0 hard cut is a verb
/// `Err`, never a `conformant: false` report); no report JSON on stdout.
#[test]
fn validate_wrong_format_version_exits_two_no_report_on_stdout() {
    let (code, stdout) = run_hdx_full(&[
        "validate",
        &fixture_arg("conformance/invalid/wrong-format-version"),
    ]);

    assert_eq!(
        code, 2,
        "the §0 hard cut surfaces as an error (exit 2), never softened into a report"
    );
    assert!(
        stdout.is_empty(),
        "an exit-2 error emits no report JSON on stdout (diagnostics go to stderr), got: {}",
        String::from_utf8_lossy(&stdout)
    );
}

/// `describe` of `invalid/wrong-format-version` → exit 2 (the §0 hard cut applies to
/// `describe` too); no JSON on stdout.
#[test]
fn describe_wrong_format_version_exits_two_no_json_on_stdout() {
    let (code, stdout) = run_hdx_full(&[
        "describe",
        &fixture_arg("conformance/invalid/wrong-format-version"),
    ]);

    assert_eq!(code, 2, "describe also hard-cuts an unknown format_version");
    assert!(
        stdout.is_empty(),
        "an exit-2 error emits no JSON on stdout, got: {}",
        String::from_utf8_lossy(&stdout)
    );
}

/// `validate` of a nonexistent path → exit 2 (`ManifestUnreadable`); no stdout.
#[test]
fn validate_nonexistent_path_exits_two() {
    let (code, stdout) = run_hdx_full(&[
        "validate",
        &fixture_arg("conformance/does-not-exist-xyz"),
    ]);

    assert_eq!(code, 2, "a nonexistent dataset path is an exit-2 error");
    assert!(
        stdout.is_empty(),
        "an exit-2 error emits no JSON on stdout, got: {}",
        String::from_utf8_lossy(&stdout)
    );
}

/// `describe` of a nonexistent path → exit 2 (`ManifestUnreadable`); no stdout.
#[test]
fn describe_nonexistent_path_exits_two() {
    let (code, stdout) = run_hdx_full(&[
        "describe",
        &fixture_arg("conformance/does-not-exist-xyz"),
    ]);

    assert_eq!(code, 2, "a nonexistent dataset path is an exit-2 error");
    assert!(
        stdout.is_empty(),
        "an exit-2 error emits no JSON on stdout, got: {}",
        String::from_utf8_lossy(&stdout)
    );
}

/// No subcommand → exit 2 (`clap` usage error).
#[test]
fn no_subcommand_exits_two_usage_error() {
    let (code, _stdout) = run_hdx_full(&[]);
    assert_eq!(code, 2, "a missing subcommand is a usage error (exit 2)");
}

/// `validate` with no path → exit 2 (`clap` usage error: missing required arg).
#[test]
fn validate_without_path_exits_two_usage_error() {
    let (code, _stdout) = run_hdx_full(&["validate"]);
    assert_eq!(
        code, 2,
        "validate with no path is a usage error (missing required arg, exit 2)"
    );
}
