//! The manifest boundary parse — raw JSON into the six-field floor (spec §11).
//!
//! [`Manifest`] is the *irreducible floor*: exactly the six non-derivable fields
//! of spec §11 — `format_version`, `name`, `created_at`, `producer_version`,
//! `crs`, `cadence` — and nothing else. Adding any derivable field (a content
//! hash, a data version, a field catalog, a basin list) is a conformance bug, so
//! it is unrepresentable here: the struct has exactly six private fields and the
//! parser rejects any extra key.
//!
//! [`Manifest::from_json`] is the only constructor and is the system boundary
//! (parse, don't validate, architecture §3): it turns a raw JSON string into a
//! valid-by-construction [`Manifest`], enforcing the §14 manifest checks that are
//! local to the manifest:
//!
//! | Check | Enforcement |
//! |---|---|
//! | M1 | valid JSON; `format_version` is read **first** |
//! | M2 | `format_version == "0.1"`; any other value is a hard cut (rejected) |
//! | M3 | exactly the six floor fields — a 7th field *and* a missing field both reject |
//! | M4 | `created_at` is strict RFC 3339; `crs`/`cadence` are non-empty |
//!
//! The cross-file checks (M5 `crs` matches file georeferencing, M6 `cadence`
//! matches the realized time axes) are **not** done here — they need dataset IO.
//!
//! Parsing proceeds in two stages. First the JSON is deserialized into a private
//! raw DTO whose `#[serde(deny_unknown_fields)]` + required `String` fields catch
//! *both* directions of M3 in one pass (a missing field and an unknown field are
//! distinct serde errors, mapped to [`CoreError::MissingManifestField`] and
//! [`CoreError::ExtraManifestField`]). Only after the DTO is in hand are the
//! field *values* interpreted — and `format_version` is hard-cut **before** any
//! other value is touched (M1/M2 ordering), so an unknown version wins over an
//! also-broken `crs`/`cadence`/`created_at`.

use std::str::FromStr;

use serde::Deserialize;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use tracing::{debug, instrument, warn};

use crate::error::CoreError;
use crate::format_version::FormatVersion;
use crate::newtypes::{Cadence, Crs, DatasetName, ProducerVersion};

/// The raw, all-string DTO the manifest JSON deserializes into.
///
/// This is the serde shape, deliberately separate from the domain [`Manifest`]:
/// keeping it raw lets the value-level checks (the hard version cut, RFC 3339,
/// non-empty strings) run *after* the structural checks, in the order spec §14
/// requires. `deny_unknown_fields` rejects a 7th field (M3 too-many); every field
/// being required (no `Option`) makes a missing field a serde error (M3 too-few).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestDto {
    format_version: String,
    name: String,
    created_at: String,
    producer_version: String,
    crs: String,
    cadence: String,
}

/// The manifest — the irreducible floor of an HDX dataset (spec §11).
///
/// Exactly the six floor fields, all private; read them through the accessors.
/// Construct one only via [`Manifest::from_json`], which validates at the
/// boundary so every constructed value is conformant with the manifest-local
/// §14 checks (M1–M4). HDX is inert and agnostic (spec §1): there is no seventh,
/// derivable field — adding one would let a declared value drift from the data
/// the floor exists to keep underivable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    format_version: FormatVersion,
    name: DatasetName,
    created_at: OffsetDateTime,
    producer_version: ProducerVersion,
    crs: Crs,
    cadence: Cadence,
}

impl Manifest {
    /// Parses a raw `manifest.json` string into a [`Manifest`] at the boundary.
    ///
    /// The parse is ordered to honor spec §0/§14: `format_version` is read and
    /// hard-cut **before** any other field value is interpreted (M1/M2), the six
    /// floor fields are enforced exactly (M3, both directions), and `created_at`
    /// is required to be strict RFC 3339 with `crs`/`cadence` non-empty (M4).
    ///
    /// # Errors
    ///
    /// | Condition | Error |
    /// |---|---|
    /// | a floor field is absent | [`CoreError::MissingManifestField`] (M3 too-few) |
    /// | a key beyond the six floor fields is present | [`CoreError::ExtraManifestField`] (M3 too-many) |
    /// | `format_version` is not `"0.1"` | [`CoreError::UnknownFormatVersion`] (M2 hard cut) — checked first |
    /// | `created_at` is not strict RFC 3339 | [`CoreError::InvalidTimestamp`] (M4) |
    /// | `crs` is empty | [`CoreError::EmptyCrs`] (M4) |
    /// | `cadence` is empty | [`CoreError::EmptyCadence`] (M4) |
    ///
    /// Invalid JSON (a syntax error or a wrong value type) is caught by the DTO
    /// deserialization in stage 1 and surfaced through [`map_serde_error`]; value
    /// conformance (version, timestamp, empties) is checked only after a
    /// structurally valid DTO is in hand.
    #[instrument(skip(json))]
    pub fn from_json(json: &str) -> Result<Self, CoreError> {
        // Stage 1 — structural parse (M1 valid JSON; M3 both directions).
        let dto: ManifestDto = serde_json::from_str(json).map_err(map_serde_error)?;

        // Stage 2a — the hard version cut, FIRST (M1/M2 ordering): an unknown
        // version is rejected before any other field value is interpreted.
        let format_version = FormatVersion::from_str(&dto.format_version)?;

        // Stage 2b — value-level conformance for the remaining fields (M4).
        let created_at = OffsetDateTime::parse(&dto.created_at, &Rfc3339).map_err(|_| {
            warn!(value = %dto.created_at, "rejecting non-RFC-3339 created_at");
            CoreError::InvalidTimestamp {
                value: dto.created_at.clone(),
            }
        })?;

        if dto.crs.is_empty() {
            warn!("rejecting empty crs");
            return Err(CoreError::EmptyCrs);
        }
        if dto.cadence.is_empty() {
            warn!("rejecting empty cadence");
            return Err(CoreError::EmptyCadence);
        }

        debug!(name = %dto.name, "parsed manifest");
        Ok(Self {
            format_version,
            name: DatasetName::new(dto.name),
            created_at,
            producer_version: ProducerVersion::new(dto.producer_version),
            crs: Crs::new(dto.crs),
            cadence: Cadence::new(dto.cadence),
        })
    }

    /// Returns the dataset's `format_version` (always [`FormatVersion::V0_1`]).
    pub fn format_version(&self) -> FormatVersion {
        self.format_version
    }

    pub fn name(&self) -> &DatasetName {
        &self.name
    }

    /// Returns the dataset creation time, parsed as strict RFC 3339.
    pub fn created_at(&self) -> OffsetDateTime {
        self.created_at
    }

    pub fn producer_version(&self) -> &ProducerVersion {
        &self.producer_version
    }

    pub fn crs(&self) -> &Crs {
        &self.crs
    }

    pub fn cadence(&self) -> &Cadence {
        &self.cadence
    }
}

/// Maps a [`serde_json`] deserialization error onto a structural [`CoreError`].
///
/// The two M3 cases are distinguished by the serde error category: a missing
/// floor field becomes [`CoreError::MissingManifestField`] and an unknown key
/// becomes [`CoreError::ExtraManifestField`], with the offending field name
/// extracted from the backtick-delimited segment of the serde message. Any other
/// deserialization failure (a JSON syntax error, a wrong value type) is surfaced
/// as [`CoreError::ExtraManifestField`] only when serde reports an unknown field;
/// otherwise it falls through to [`CoreError::MissingManifestField`] with the
/// raw serde message as the field detail, so no failure is ever swallowed.
fn map_serde_error(err: serde_json::Error) -> CoreError {
    let message = err.to_string();

    if let Some(field) = extract_backticked(&message, "missing field") {
        warn!(field = %field, "rejecting manifest with a missing floor field");
        return CoreError::MissingManifestField { field };
    }
    if let Some(field) = extract_backticked(&message, "unknown field") {
        warn!(field = %field, "rejecting manifest with an unexpected field");
        return CoreError::ExtraManifestField { field };
    }

    // Any other deserialization failure (JSON syntax, wrong value type): not a
    // structural floor-field violation. Report it verbatim through the closest
    // structural variant so it surfaces rather than being silently dropped.
    warn!(error = %message, "rejecting unparsable manifest JSON");
    CoreError::MissingManifestField { field: message }
}

/// Extracts the name inside the first pair of backticks following `prefix`.
///
/// serde messages read `missing field \`cadence\`` and
/// `unknown field \`content_hash\`, expected one of …`. This returns the
/// backticked name when `message` starts with `prefix`, else `None`.
fn extract_backticked(message: &str, prefix: &str) -> Option<String> {
    if !message.starts_with(prefix) {
        return None;
    }
    let after = message.find('`')? + 1;
    let len = message[after..].find('`')?;
    Some(message[after..after + len].to_string())
}

#[cfg(test)]
mod tests {
    use crate::error::CoreError;
    use crate::format_version::FormatVersion;
    use crate::manifest::Manifest;

    /// The exact six-field example manifest from spec §11.
    const SPEC_EXAMPLE: &str = r#"{
  "format_version": "0.1",
  "name": "<dataset name>",
  "created_at": "2026-06-01T00:00:00Z",
  "producer_version": "<tool/version that wrote it>",
  "crs": "EPSG:4326",
  "cadence": "daily"
}"#;

    #[test]
    fn spec_example_parses_and_accessors_round_trip() {
        let manifest = Manifest::from_json(SPEC_EXAMPLE).expect("the §11 example must parse");
        assert_eq!(manifest.format_version(), FormatVersion::V0_1);
        assert_eq!(manifest.name().as_str(), "<dataset name>");
        assert_eq!(
            manifest.producer_version().as_str(),
            "<tool/version that wrote it>"
        );
        assert_eq!(manifest.crs().as_str(), "EPSG:4326");
        assert_eq!(manifest.cadence().as_str(), "daily");
        // `created_at` parsed to the expected instant (Z form → UTC).
        assert_eq!(manifest.created_at().year(), 2026);
        assert_eq!(manifest.created_at().unix_timestamp(), 1_780_272_000);
    }

    #[test]
    fn manifest_round_trips_through_clone_and_eq() {
        let manifest = Manifest::from_json(SPEC_EXAMPLE).expect("parse");
        assert_eq!(manifest.clone(), manifest);
    }

    #[test]
    fn seventh_field_is_rejected_as_extra_m3_too_many() {
        // The six floor fields plus a derivable `content_hash` (spec §0/§11
        // forbid it) — M3 "too-many" direction.
        let json = r#"{
            "format_version": "0.1",
            "name": "ds",
            "created_at": "2026-06-01T00:00:00Z",
            "producer_version": "tool/1.0",
            "crs": "EPSG:4326",
            "cadence": "daily",
            "content_hash": "deadbeef"
        }"#;
        match Manifest::from_json(json) {
            Err(CoreError::ExtraManifestField { field }) => {
                assert_eq!(field, "content_hash");
            }
            other => panic!("expected ExtraManifestField, got {other:?}"),
        }
    }

    #[test]
    fn five_field_manifest_is_rejected_as_missing_m3_too_few() {
        // Omit `cadence` — M3 "too-few" direction.
        let json = r#"{
            "format_version": "0.1",
            "name": "ds",
            "created_at": "2026-06-01T00:00:00Z",
            "producer_version": "tool/1.0",
            "crs": "EPSG:4326"
        }"#;
        match Manifest::from_json(json) {
            Err(CoreError::MissingManifestField { field }) => {
                assert_eq!(field, "cadence");
            }
            other => panic!("expected MissingManifestField{{field:\"cadence\"}}, got {other:?}"),
        }
    }

    #[test]
    fn unknown_format_version_wins_before_other_fields_validated() {
        // `format_version` is read and hard-cut FIRST (M1/M2): even though `crs`
        // is also empty here, the version error must be the one returned.
        let json = r#"{
            "format_version": "0.2",
            "name": "ds",
            "created_at": "2026-06-01T00:00:00Z",
            "producer_version": "tool/1.0",
            "crs": "",
            "cadence": "daily"
        }"#;
        match Manifest::from_json(json) {
            Err(CoreError::UnknownFormatVersion { found }) => {
                assert_eq!(
                    found, "0.2",
                    "the version error must win over the empty crs"
                );
            }
            other => panic!("expected UnknownFormatVersion (read first), got {other:?}"),
        }
    }

    #[test]
    fn date_only_created_at_is_rejected_as_invalid_timestamp() {
        // A date-only value is not RFC 3339 (no time/offset) — M4.
        let json = r#"{
            "format_version": "0.1",
            "name": "ds",
            "created_at": "2026-06-01",
            "producer_version": "tool/1.0",
            "crs": "EPSG:4326",
            "cadence": "daily"
        }"#;
        match Manifest::from_json(json) {
            Err(CoreError::InvalidTimestamp { value }) => assert_eq!(value, "2026-06-01"),
            other => panic!("expected InvalidTimestamp, got {other:?}"),
        }
    }

    #[test]
    fn garbage_created_at_is_rejected_as_invalid_timestamp() {
        let json = r#"{
            "format_version": "0.1",
            "name": "ds",
            "created_at": "not-a-date",
            "producer_version": "tool/1.0",
            "crs": "EPSG:4326",
            "cadence": "daily"
        }"#;
        match Manifest::from_json(json) {
            Err(CoreError::InvalidTimestamp { value }) => assert_eq!(value, "not-a-date"),
            other => panic!("expected InvalidTimestamp, got {other:?}"),
        }
    }

    #[test]
    fn z_form_and_offset_form_created_at_both_parse() {
        // The §11 example uses the `Z` (UTC) form; an explicit offset is also
        // valid RFC 3339 and must parse (M4).
        for ts in ["2026-06-01T00:00:00Z", "2026-06-01T02:00:00+02:00"] {
            let json = format!(
                r#"{{
                    "format_version": "0.1",
                    "name": "ds",
                    "created_at": "{ts}",
                    "producer_version": "tool/1.0",
                    "crs": "EPSG:4326",
                    "cadence": "daily"
                }}"#
            );
            let manifest =
                Manifest::from_json(&json).unwrap_or_else(|e| panic!("{ts:?} must parse: {e:?}"));
            // Both denote the same instant: 2026-06-01T00:00:00 UTC.
            assert_eq!(manifest.created_at().unix_timestamp(), 1_780_272_000);
        }
    }

    #[test]
    fn empty_crs_is_rejected() {
        let json = r#"{
            "format_version": "0.1",
            "name": "ds",
            "created_at": "2026-06-01T00:00:00Z",
            "producer_version": "tool/1.0",
            "crs": "",
            "cadence": "daily"
        }"#;
        match Manifest::from_json(json) {
            Err(CoreError::EmptyCrs) => {}
            other => panic!("expected EmptyCrs, got {other:?}"),
        }
    }

    #[test]
    fn empty_cadence_is_rejected() {
        let json = r#"{
            "format_version": "0.1",
            "name": "ds",
            "created_at": "2026-06-01T00:00:00Z",
            "producer_version": "tool/1.0",
            "crs": "EPSG:4326",
            "cadence": ""
        }"#;
        match Manifest::from_json(json) {
            Err(CoreError::EmptyCadence) => {}
            other => panic!("expected EmptyCadence, got {other:?}"),
        }
    }

    #[test]
    fn malformed_json_is_rejected_without_panicking() {
        // Not valid JSON at all — must return a typed error, never panic (M1).
        match Manifest::from_json("{ this is not json }") {
            Err(_) => {}
            Ok(m) => panic!("malformed JSON must not parse, got {m:?}"),
        }
    }
}
