//! Cross-check the committed manifest JSON Schema against the S4 boundary parser.
//!
//! This pins the *manifest half* of R4 (architecture §7): the schema asset in
//! `schemas/manifest.schema.json` and [`Manifest::from_json`] are two encodings
//! of the same §11 floor, and they must agree. The test asserts agreement in
//! both M3 directions (spec §14 M3, folding critique MED-3):
//!
//! - the §11 example validates against the schema (and parses);
//! - a 7-field manifest is rejected by the schema (`additionalProperties:false`,
//!   M3 too-many) **and** by the parser ([`CoreError::ExtraManifestField`]);
//! - a 5-field manifest is rejected by the schema (`required` lists all six,
//!   M3 too-few) **and** by the parser ([`CoreError::MissingManifestField`]).
//!
//! `jsonschema` is a `[dev-dependencies]` entry — test-only, never shipped in
//! `hdx-core`; no production path depends on it.

use std::path::PathBuf;

use jsonschema::Validator;
use serde_json::Value;

use hdx_core::error::CoreError;
use hdx_core::manifest::Manifest;

/// The exact six-field example manifest from spec §11.
const SPEC_EXAMPLE: &str = r#"{
  "format_version": "0.1",
  "name": "<dataset name>",
  "created_at": "2026-06-01T00:00:00Z",
  "producer_version": "<tool/version that wrote it>",
  "crs": "EPSG:4326",
  "cadence": "daily"
}"#;

/// The six floor fields plus a derivable `content_hash` — M3 "too-many" (§11/§14).
const SEVEN_FIELD: &str = r#"{
  "format_version": "0.1",
  "name": "ds",
  "created_at": "2026-06-01T00:00:00Z",
  "producer_version": "tool/1.0",
  "crs": "EPSG:4326",
  "cadence": "daily",
  "content_hash": "deadbeef"
}"#;

/// The floor with `cadence` omitted — M3 "too-few" (§11/§14, folds MED-3).
const FIVE_FIELD: &str = r#"{
  "format_version": "0.1",
  "name": "ds",
  "created_at": "2026-06-01T00:00:00Z",
  "producer_version": "tool/1.0",
  "crs": "EPSG:4326"
}"#;

/// Loads and compiles the committed `schemas/manifest.schema.json`.
///
/// The schema lives at the repository root; `CARGO_MANIFEST_DIR` points at
/// `crates/core`, so we climb two directories to reach it.
fn load_schema() -> Validator {
    let schema_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../schemas/manifest.schema.json");
    let raw = std::fs::read_to_string(&schema_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", schema_path.display()));
    let schema: Value =
        serde_json::from_str(&raw).expect("manifest.schema.json must be valid JSON");
    jsonschema::validator_for(&schema).expect("manifest.schema.json must compile as a JSON Schema")
}

/// Parses a manifest fixture into a `serde_json::Value` for schema validation.
fn as_value(json: &str) -> Value {
    serde_json::from_str(json).expect("fixture must be valid JSON")
}

#[test]
fn spec_example_validates_against_schema_and_parses() {
    let validator = load_schema();
    assert!(
        validator.is_valid(&as_value(SPEC_EXAMPLE)),
        "the §11 example must validate against the committed schema"
    );
    Manifest::from_json(SPEC_EXAMPLE).expect("the §11 example must parse via the S4 parser");
}

#[test]
fn seven_field_manifest_rejected_by_schema_and_parser_m3_too_many() {
    let validator = load_schema();
    // Schema half: `additionalProperties: false` rejects the 7th field.
    assert!(
        !validator.is_valid(&as_value(SEVEN_FIELD)),
        "a 7-field manifest must fail schema validation (M3 too-many)"
    );
    // Parser half: the same shape is `ExtraManifestField`.
    match Manifest::from_json(SEVEN_FIELD) {
        Err(CoreError::ExtraManifestField { field }) => assert_eq!(field, "content_hash"),
        other => panic!("expected ExtraManifestField from the parser, got {other:?}"),
    }
}

#[test]
fn five_field_manifest_rejected_by_schema_and_parser_m3_too_few() {
    let validator = load_schema();
    // Schema half: `required` lists all six, so an omitted field fails (MED-3).
    assert!(
        !validator.is_valid(&as_value(FIVE_FIELD)),
        "a 5-field manifest must fail schema validation (M3 too-few — MED-3)"
    );
    // Parser half: the same shape is `MissingManifestField`.
    match Manifest::from_json(FIVE_FIELD) {
        Err(CoreError::MissingManifestField { field }) => assert_eq!(field, "cadence"),
        other => panic!("expected MissingManifestField from the parser, got {other:?}"),
    }
}
