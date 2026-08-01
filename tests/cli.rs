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
//! - **S3 schema conformance** — the binary's stdout, parsed through the real
//!   process boundary, validates against the committed `schemas/describe.schema.json`
//!   / `schemas/validate.schema.json` (R4), proving the CLI is a faithful,
//!   non-reshaping surface over the MS5/MS6 wire shape and never re-derives it.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use jsonschema::Validator;
use serde_json::Value;

/// Resolve a fixture dir relative to the bin crate root (the workspace root).
fn fixture(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel)
}

fn copy_dir_all(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).expect("create copied fixture directory");
    for entry in fs::read_dir(src).expect("read fixture directory") {
        let entry = entry.expect("read fixture entry");
        let file_type = entry.file_type().expect("read fixture entry type");
        let destination = dst.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_all(&entry.path(), &destination);
        } else {
            fs::copy(entry.path(), destination).expect("copy fixture file");
        }
    }
}

fn copy_fixture_to_temp(rel: &str, tag: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time after unix epoch")
        .as_nanos();
    let destination =
        std::env::temp_dir().join(format!("hdx-cli-{tag}-{}-{unique}", std::process::id()));
    copy_dir_all(&fixture(rel), &destination);
    destination
}

fn set_consolidated_time_encoding(store: &Path, units: &str, calendar: &str) {
    let root_metadata = store.join("zarr.json");
    let mut root: Value =
        serde_json::from_slice(&fs::read(&root_metadata).expect("read copied root zarr metadata"))
            .expect("parse copied root zarr metadata");
    let attributes = root
        .pointer_mut("/consolidated_metadata/metadata/time/attributes")
        .and_then(Value::as_object_mut)
        .expect("inline consolidated time attributes");
    attributes.insert("units".to_string(), Value::String(units.to_string()));
    attributes.insert("calendar".to_string(), Value::String(calendar.to_string()));
    fs::write(
        root_metadata,
        serde_json::to_vec(&root).expect("serialize copied root zarr metadata"),
    )
    .expect("write copied root zarr metadata");
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
    let stdout = run_hdx(&[
        "describe",
        path.to_str().expect("fixture path is valid UTF-8"),
    ]);

    assert!(!stdout.is_empty(), "describe produced empty stdout");

    let value: Value = serde_json::from_slice(&stdout).expect("describe stdout is not valid JSON");
    assert!(value.is_object(), "describe stdout is not a JSON object");
}

#[test]
fn describe_surfaces_each_store_declared_gridded_time_encoding() {
    let root = copy_fixture_to_temp(
        "conformance/valid/multi_grid_multi_static",
        "declared-time-encoding",
    );
    for basin_id in ["0001", "0002", "0003"] {
        for label in ["era5", "merit"] {
            set_consolidated_time_encoding(
                &root.join(format!("basin={basin_id}/gridded_dynamic/{label}.zarr")),
                "days since 1900-01-01",
                "gregorian",
            );
        }
    }

    let root_arg = root
        .to_str()
        .expect("temporary fixture path is valid UTF-8");
    let (code, stdout) = run_hdx_full(&["describe", root_arg]);
    fs::remove_dir_all(&root).expect("remove copied fixture");

    assert_eq!(code, 0, "describe of the copied fixture must exit 0");
    let value: Value = serde_json::from_slice(&stdout).expect("describe stdout is valid JSON");
    let extents = value
        .get("time_extents")
        .and_then(Value::as_array)
        .expect("time_extents array");
    assert_eq!(extents.len(), 3, "one time extent per basin");

    for (entry, expected_basin_id) in extents.iter().zip(["0001", "0002", "0003"]) {
        assert_eq!(
            entry.get("basin_id").and_then(Value::as_str),
            Some(expected_basin_id)
        );
        let axis = entry
            .get("gridded_time_axis")
            .expect("gridded basin carries its time axis");
        assert_eq!(
            axis.get("units").and_then(Value::as_str),
            Some("days since 1900-01-01")
        );
        assert_eq!(
            axis.get("calendar").and_then(Value::as_str),
            Some("gregorian")
        );
        let micros = axis
            .get("micros")
            .and_then(Value::as_array)
            .expect("gridded micros array");
        assert!(!micros.is_empty(), "gridded micros array is non-empty");
        if expected_basin_id == "0001" {
            assert_eq!(micros[0].as_i64(), Some(946_684_800_000_000));
        }
    }
}

#[test]
fn validate_emits_report_object_with_conformant_key() {
    let path = fixture("conformance/valid/minimal");
    let stdout = run_hdx(&[
        "validate",
        path.to_str().expect("fixture path is valid UTF-8"),
    ]);

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

#[test]
fn validate_unpinned_gridded_time_units_exits_one_and_names_units() {
    let (code, stdout) = run_hdx_full(&[
        "validate",
        &fixture_arg("conformance/invalid/unpinned-gridded-time-units"),
    ]);
    let report: Value = serde_json::from_slice(&stdout).expect("validate stdout is valid JSON");
    assert_eq!(
        (code, report.get("conformant").and_then(Value::as_bool)),
        (1, Some(false)),
        "unpinned gridded time units must be a non-conformant report: {report}"
    );

    let checks = report
        .get("checks")
        .and_then(Value::as_array)
        .expect("checks array");
    let t3 = checks
        .iter()
        .find(|check| check.get("id").and_then(Value::as_str) == Some("T3"))
        .expect("T3 present");
    assert_eq!(
        t3,
        &serde_json::json!({
            "id": "T3",
            "status": "ran",
            "result": "fail",
            "depth": "metadata_deep",
            "detail": "basin \"0003\" gridded_dynamic/era5.zarr standalone time units \"days since 1900-01-01\"; expected \"days since 1970-01-01\" | basin \"0003\" gridded_dynamic/era5.zarr consolidated time units \"days since 1900-01-01\"; expected \"days since 1970-01-01\""
        })
    );
    let t2 = checks
        .iter()
        .find(|check| check.get("id").and_then(Value::as_str) == Some("T2"))
        .expect("T2 present");
    assert_eq!(t2.get("status").and_then(Value::as_str), Some("ran"));
    assert_eq!(t2.get("result").and_then(Value::as_str), Some("pass"));
}

#[test]
fn validate_divergent_standalone_time_units_exits_one_and_names_site() {
    let (code, stdout) = run_hdx_full(&[
        "validate",
        &fixture_arg("conformance/invalid/divergent-standalone-gridded-time-units"),
    ]);
    let report: Value = serde_json::from_slice(&stdout).expect("validate stdout is valid JSON");
    assert_eq!(
        (code, report.get("conformant").and_then(Value::as_bool)),
        (1, Some(false)),
        "a non-pinned standalone declaration must fail closed: {report}"
    );
    let detail = report
        .get("checks")
        .and_then(Value::as_array)
        .and_then(|checks| {
            checks
                .iter()
                .find(|check| check.get("id").and_then(Value::as_str) == Some("T3"))
        })
        .and_then(|check| check.get("detail"))
        .and_then(Value::as_str)
        .expect("T3 failure detail");
    assert!(
        detail.contains("standalone time units"),
        "site is named: {detail}"
    );
    assert!(
        detail.contains("days since 1800-01-01"),
        "value is named: {detail}"
    );
    assert!(
        detail.contains("time units divergence")
            && detail.contains("consolidated \"days since 1970-01-01\""),
        "divergence and both sites are named: {detail}"
    );
}

/// `validate` of `valid/geometry-less` → exit 0, stdout `"conformant": true` (0.2).
///
/// The geometry-optional 0.2 fixture (scalar_static present, outlines absent) validates
/// conformant through the real binary — the CLI transports the `validate_json` verdict
/// verbatim, so a 0.2-conformant geometry-less dataset exits 0 just like `valid/minimal`.
#[test]
fn validate_geometry_less_exits_zero_conformant_true_under_0_2() {
    let (code, stdout) =
        run_hdx_full(&["validate", &fixture_arg("conformance/valid/geometry-less")]);

    assert_eq!(
        code, 0,
        "a 0.2-conformant geometry-less dataset must exit 0"
    );
    let value: Value = serde_json::from_slice(&stdout).expect("validate stdout is valid JSON");
    assert_eq!(
        value.get("conformant").and_then(Value::as_bool),
        Some(true),
        "the geometry-less fixture reports conformant: true under 0.2"
    );
    // Geo1 skipped (no outlines), every other check ran:pass — no fail flips the verdict.
    let checks = value
        .get("checks")
        .and_then(Value::as_array)
        .expect("checks array");
    let geo1 = checks
        .iter()
        .find(|c| c.get("id").and_then(Value::as_str) == Some("Geo1"))
        .expect("Geo1 present");
    assert_eq!(
        geo1.get("status").and_then(Value::as_str),
        Some("skipped"),
        "Geo1 skips when outlines is absent (the 0.2 relaxation)"
    );
}

/// `describe` of `valid/geometry-less` → exit 0, a JSON object with empty delineations.
#[test]
fn describe_geometry_less_exits_zero_empty_delineations() {
    let (code, stdout) =
        run_hdx_full(&["describe", &fixture_arg("conformance/valid/geometry-less")]);

    assert_eq!(
        code, 0,
        "describe of a 0.2 geometry-less dataset must exit 0"
    );
    let value: Value = serde_json::from_slice(&stdout).expect("describe stdout is valid JSON");
    assert_eq!(
        value
            .get("manifest")
            .and_then(|m| m.get("format_version"))
            .and_then(Value::as_str),
        Some("0.2"),
        "the geometry-less manifest is format_version 0.2"
    );
    assert_eq!(
        value
            .get("delineations")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(0),
        "a geometry-less dataset has empty delineations (no outlines)"
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

/// A dynamic-Zarr declaration/coordinate disagreement is a reader error, so
/// `validate` exits 2 and emits no softened conformance report on stdout.
#[test]
fn validate_grid_resolution_mismatch_exits_two_no_report_on_stdout() {
    let (code, stdout) = run_hdx_full(&[
        "validate",
        &fixture_arg("conformance/invalid/grid-resolution-mismatch"),
    ]);

    assert_eq!(code, 2, "grid-resolution mismatch is a reader error");
    assert!(
        stdout.is_empty(),
        "an exit-2 reader error emits no report JSON on stdout, got: {}",
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
    let (code, stdout) =
        run_hdx_full(&["validate", &fixture_arg("conformance/does-not-exist-xyz")]);

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
    let (code, stdout) =
        run_hdx_full(&["describe", &fixture_arg("conformance/does-not-exist-xyz")]);

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

// --- MS7-S3: schema conformance through the process boundary (R4) ------------------
//
// The CLI must honor the MS5/MS6 wire contract *verbatim* — it serializes the verb's
// returned value and prints it; it never re-derives the shape (architecture §2). These
// tests run the real binary, parse its stdout as JSON, compile the committed schema with
// the test-only `jsonschema` dev-dep (the same mechanism the `hdx-core` tests use), and
// assert the stdout validates. Validation failures panic with the `jsonschema` error so a
// drift between the verb's serializer and the committed schema is debuggable.

/// Resolve a committed schema path relative to the bin crate root (the workspace root).
fn schema(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("schemas")
        .join(rel)
}

/// Load and compile a committed JSON Schema with `jsonschema`.
///
/// Panics if the schema file is unreadable, is not valid JSON, or does not compile as a
/// JSON Schema — each is a broken committed contract, not a flaky test.
fn load_schema(file: &str) -> Validator {
    let path = schema(file);
    let raw =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let document: Value =
        serde_json::from_str(&raw).unwrap_or_else(|e| panic!("{file} must be valid JSON: {e}"));
    jsonschema::validator_for(&document)
        .unwrap_or_else(|e| panic!("{file} must compile as a JSON Schema: {e}"))
}

/// Parse a process's stdout bytes as a JSON `Value`, panicking with context on failure.
fn stdout_as_json(stdout: &[u8], what: &str) -> Value {
    serde_json::from_slice(stdout)
        .unwrap_or_else(|e| panic!("{what} stdout is not valid JSON: {e}"))
}

/// `describe` stdout validates against the committed `describe.schema.json` (R4).
///
/// The CLI prints the MS5 `Description` JSON verbatim; this asserts the surface does not
/// reshape it on the way to stdout.
#[test]
fn describe_stdout_validates_against_describe_schema() {
    let validator = load_schema("describe.schema.json");
    let stdout = run_hdx(&["describe", &fixture_arg("conformance/valid/minimal")]);
    let value = stdout_as_json(&stdout, "describe");

    if let Err(error) = validator.validate(&value) {
        panic!("describe stdout must validate against describe.schema.json: {error}");
    }
}

/// `validate` stdout (conformant case) validates against `validate.schema.json` (R4).
///
/// The CLI prints the MS6 `ValidationReport` JSON verbatim; this asserts the surface does
/// not reshape it on the way to stdout.
#[test]
fn validate_stdout_validates_against_validate_schema() {
    let validator = load_schema("validate.schema.json");
    let stdout = run_hdx(&["validate", &fixture_arg("conformance/valid/minimal")]);
    let value = stdout_as_json(&stdout, "validate");

    if let Err(error) = validator.validate(&value) {
        panic!("validate stdout must validate against validate.schema.json: {error}");
    }
}

/// `validate`/`describe` stdout for the geometry-less (0.2) fixture validate against the
/// committed schemas (R4): the widened `format_version` enum + the additive
/// `gridded_time_axis` def must accept the geometry-less wire shape through the binary.
#[test]
fn geometry_less_stdout_validates_against_schemas() {
    let validate_validator = load_schema("validate.schema.json");
    let stdout = run_hdx(&["validate", &fixture_arg("conformance/valid/geometry-less")]);
    let value = stdout_as_json(&stdout, "validate (geometry-less)");
    if let Err(error) = validate_validator.validate(&value) {
        panic!("geometry-less validate stdout must validate against validate.schema.json: {error}");
    }

    let describe_validator = load_schema("describe.schema.json");
    let stdout = run_hdx(&["describe", &fixture_arg("conformance/valid/geometry-less")]);
    let value = stdout_as_json(&stdout, "describe (geometry-less)");
    if let Err(error) = describe_validator.validate(&value) {
        panic!("geometry-less describe stdout must validate against describe.schema.json: {error}");
    }
}

/// A `conformant: false` report still validates against `validate.schema.json` (R4).
///
/// `validate conformance/invalid/missing-root-rollup` exits 1 with a non-conformant
/// report — but a non-conformant verdict is still a *well-formed report*, so its stdout
/// must validate against the same schema. This pins that exit-1 output is schema-faithful,
/// not a degraded shape.
#[test]
fn nonconformant_validate_stdout_still_validates_against_schema() {
    let validator = load_schema("validate.schema.json");
    let (code, stdout) = run_hdx_full(&[
        "validate",
        &fixture_arg("conformance/invalid/missing-root-rollup"),
    ]);

    assert_eq!(
        code, 1,
        "the missing-root-rollup fixture is non-conformant (exit 1)"
    );
    let value = stdout_as_json(&stdout, "validate (non-conformant)");
    assert_eq!(
        value.get("conformant").and_then(Value::as_bool),
        Some(false),
        "this fixture's report must carry conformant: false"
    );

    if let Err(error) = validator.validate(&value) {
        panic!(
            "a conformant:false report must still validate against validate.schema.json: {error}"
        );
    }
}
