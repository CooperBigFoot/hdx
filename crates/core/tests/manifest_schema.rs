//! Cross-check the committed manifest JSON Schema against the S4 boundary parser.
//!
//! This pins the *manifest half* of R4 (architecture §7): the schema asset in
//! `schemas/manifest.schema.json` and [`Manifest::from_json`] are two encodings
//! of the same §11 manifest contract, and they must agree. The test asserts
//! agreement for the optional consumer ABI and in both M3 directions:
//!
//! - the §11 example validates against the schema (and parses);
//! - valid channel declarations validate, parse, and preserve consumer order;
//! - malformed declarations are rejected with dedicated typed parser errors;
//! - an unknown derivable property is rejected by the schema
//!   (`additionalProperties:false`) and parser ([`CoreError::ExtraManifestField`]);
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
use hdx_core::newtypes::GridLabel;

/// The exact six-field example manifest from spec §11.
const SPEC_EXAMPLE: &str = r#"{
  "format_version": "0.1",
  "name": "<dataset name>",
  "created_at": "2026-06-01T00:00:00Z",
  "producer_version": "<tool/version that wrote it>",
  "crs": "EPSG:4326",
  "cadence": "daily"
}"#;

/// The six floor fields plus an unknown derivable `content_hash` (§11/§14 M3).
const UNKNOWN_DERIVABLE_FIELD: &str = r#"{
  "format_version": "0.1",
  "name": "ds",
  "created_at": "2026-06-01T00:00:00Z",
  "producer_version": "tool/1.0",
  "crs": "EPSG:4326",
  "cadence": "daily",
  "content_hash": "deadbeef"
}"#;

const VALID_MULTI_LABEL: &str = r#"{
  "format_version": "0.1",
  "name": "ds",
  "created_at": "2026-06-01T00:00:00Z",
  "producer_version": "tool/1.0",
  "crs": "EPSG:4326",
  "cadence": "daily",
  "gridded_static_channels": {
    "era5": ["elevation", "soil_depth"],
    "landcover": ["forest_fraction", "urban_fraction"]
  }
}"#;

const PRESENT_EMPTY_OBJECT: &str = r#"{
  "format_version": "0.1",
  "name": "ds",
  "created_at": "2026-06-01T00:00:00Z",
  "producer_version": "tool/1.0",
  "crs": "EPSG:4326",
  "cadence": "daily",
  "gridded_static_channels": {}
}"#;

const EMPTY_LABEL: &str = r#"{
  "format_version": "0.1",
  "name": "ds",
  "created_at": "2026-06-01T00:00:00Z",
  "producer_version": "tool/1.0",
  "crs": "EPSG:4326",
  "cadence": "daily",
  "gridded_static_channels": {"": ["elevation"]}
}"#;

const EMPTY_LIST: &str = r#"{
  "format_version": "0.1",
  "name": "ds",
  "created_at": "2026-06-01T00:00:00Z",
  "producer_version": "tool/1.0",
  "crs": "EPSG:4326",
  "cadence": "daily",
  "gridded_static_channels": {"era5": []}
}"#;

const EMPTY_NAME: &str = r#"{
  "format_version": "0.1",
  "name": "ds",
  "created_at": "2026-06-01T00:00:00Z",
  "producer_version": "tool/1.0",
  "crs": "EPSG:4326",
  "cadence": "daily",
  "gridded_static_channels": {"era5": ["elevation", ""]}
}"#;

const DUPLICATE_NAME: &str = r#"{
  "format_version": "0.1",
  "name": "ds",
  "created_at": "2026-06-01T00:00:00Z",
  "producer_version": "tool/1.0",
  "crs": "EPSG:4326",
  "cadence": "daily",
  "gridded_static_channels": {"era5": ["elevation", "elevation"]}
}"#;

const NON_OBJECT: &str = r#"{
  "format_version": "0.1",
  "name": "ds",
  "created_at": "2026-06-01T00:00:00Z",
  "producer_version": "tool/1.0",
  "crs": "EPSG:4326",
  "cadence": "daily",
  "gridded_static_channels": []
}"#;

const NON_ARRAY: &str = r#"{
  "format_version": "0.1",
  "name": "ds",
  "created_at": "2026-06-01T00:00:00Z",
  "producer_version": "tool/1.0",
  "crs": "EPSG:4326",
  "cadence": "daily",
  "gridded_static_channels": {"era5": "elevation"}
}"#;

const NON_STRING: &str = r#"{
  "format_version": "0.1",
  "name": "ds",
  "created_at": "2026-06-01T00:00:00Z",
  "producer_version": "tool/1.0",
  "crs": "EPSG:4326",
  "cadence": "daily",
  "gridded_static_channels": {"era5": ["elevation", 1]}
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
    let manifest =
        Manifest::from_json(SPEC_EXAMPLE).expect("the §11 example must parse via the S4 parser");
    assert!(manifest.gridded_static_channels().is_empty());
}

#[test]
fn valid_multi_label_channels_validate_parse_and_preserve_order() {
    let validator = load_schema();
    assert!(validator.is_valid(&as_value(VALID_MULTI_LABEL)));
    let manifest = Manifest::from_json(VALID_MULTI_LABEL).expect("valid channels must parse");
    let era5 = manifest
        .gridded_static_channels()
        .get(&GridLabel::new("era5"))
        .expect("era5 declaration");
    assert_eq!(
        era5.iter().map(|name| name.as_str()).collect::<Vec<_>>(),
        ["elevation", "soil_depth"]
    );
    let landcover = manifest
        .gridded_static_channels()
        .get(&GridLabel::new("landcover"))
        .expect("landcover declaration");
    assert_eq!(
        landcover
            .iter()
            .map(|name| name.as_str())
            .collect::<Vec<_>>(),
        ["forest_fraction", "urban_fraction"]
    );
}

#[test]
fn present_empty_channel_object_is_an_empty_declaration() {
    let validator = load_schema();
    assert!(validator.is_valid(&as_value(PRESENT_EMPTY_OBJECT)));
    let manifest = Manifest::from_json(PRESENT_EMPTY_OBJECT).expect("empty object must parse");
    assert!(manifest.gridded_static_channels().is_empty());
}

#[test]
fn unknown_derivable_field_rejected_by_schema_and_parser_m3_too_many() {
    let validator = load_schema();
    assert!(
        !validator.is_valid(&as_value(UNKNOWN_DERIVABLE_FIELD)),
        "an unknown derivable property must fail schema validation"
    );
    match Manifest::from_json(UNKNOWN_DERIVABLE_FIELD) {
        Err(CoreError::ExtraManifestField { field }) => assert_eq!(field, "content_hash"),
        other => panic!("expected ExtraManifestField from the parser, got {other:?}"),
    }
}

#[test]
fn empty_label_rejected_by_schema_and_parser() {
    let validator = load_schema();
    assert!(!validator.is_valid(&as_value(EMPTY_LABEL)));
    assert!(matches!(
        Manifest::from_json(EMPTY_LABEL),
        Err(CoreError::EmptyGriddedStaticChannelLabel)
    ));
}

#[test]
fn empty_list_rejected_by_schema_and_parser() {
    let validator = load_schema();
    assert!(!validator.is_valid(&as_value(EMPTY_LIST)));
    match Manifest::from_json(EMPTY_LIST) {
        Err(CoreError::EmptyGriddedStaticChannelList { label }) => assert_eq!(label, "era5"),
        other => panic!("expected EmptyGriddedStaticChannelList, got {other:?}"),
    }
}

#[test]
fn empty_name_rejected_by_schema_and_parser() {
    let validator = load_schema();
    assert!(!validator.is_valid(&as_value(EMPTY_NAME)));
    match Manifest::from_json(EMPTY_NAME) {
        Err(CoreError::EmptyGriddedStaticChannelName { label, index }) => {
            assert_eq!(label, "era5");
            assert_eq!(index, 1);
        }
        other => panic!("expected EmptyGriddedStaticChannelName, got {other:?}"),
    }
}

#[test]
fn duplicate_name_rejected_by_schema_and_parser() {
    let validator = load_schema();
    assert!(!validator.is_valid(&as_value(DUPLICATE_NAME)));
    match Manifest::from_json(DUPLICATE_NAME) {
        Err(CoreError::DuplicateGriddedStaticChannelName { label, name }) => {
            assert_eq!(label, "era5");
            assert_eq!(name, "elevation");
        }
        other => panic!("expected DuplicateGriddedStaticChannelName, got {other:?}"),
    }
}

fn assert_invalid_shape(fixture: &str, expected_context: &str) {
    let validator = load_schema();
    assert!(!validator.is_valid(&as_value(fixture)));
    match Manifest::from_json(fixture) {
        Err(CoreError::InvalidGriddedStaticChannelsShape { detail }) => {
            assert!(!detail.is_empty());
            assert!(
                detail.contains(expected_context),
                "shape detail {detail:?} must identify {expected_context:?}"
            );
        }
        other => panic!("expected InvalidGriddedStaticChannelsShape, got {other:?}"),
    }
}

#[test]
fn non_object_channels_rejected_by_schema_and_parser() {
    assert_invalid_shape(NON_OBJECT, "object");
}

#[test]
fn non_array_channel_value_rejected_by_schema_and_parser() {
    assert_invalid_shape(NON_ARRAY, "era5");
}

#[test]
fn non_string_channel_name_rejected_by_schema_and_parser() {
    assert_invalid_shape(NON_STRING, "index 1");
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
