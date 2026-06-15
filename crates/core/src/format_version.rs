//! The one contract-version axis for HDX, encoded as a hard version cut.
//!
//! [`FormatVersion`] enumerates the contract versions this build implements:
//! [`FormatVersion::V0_1`] and [`FormatVersion::V0_2`] (the geometry-optional
//! relaxation). Parsing a raw `format_version` string succeeds **only** on
//! `"0.1"` or `"0.2"` and errors on anything else (spec §0/§14 M2): the version
//! is a hard cut, read before any other field and rejected outright when
//! unknown. A parsed value is, by construction, one of the versions this build
//! understands — an unknown string is never representable as a [`FormatVersion`].
//!
//! The parse is exact-string (`"0.10"` is not `"0.1"`); HDX performs no numeric
//! coercion.

use std::fmt;
use std::str::FromStr;

use tracing::warn;

use crate::error::CoreError;

/// The `format_version` string for the 0.1 contract.
const V0_1_STR: &str = "0.1";

/// The `format_version` string for the 0.2 contract (geometry-optional).
const V0_2_STR: &str = "0.2";

/// The contract-version axis HDX recognizes (spec §0/§11).
///
/// The closed arm set makes the hard cut a type-level invariant: a value of this
/// type is, by construction, one of the versions this build implements. Parsing
/// is the only way to obtain one, and parsing rejects every string but `"0.1"`
/// and `"0.2"`, so no reader for an unknown version can exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatVersion {
    /// HDX `format_version = "0.1"` — the original geometry-mandatory contract.
    V0_1,
    /// HDX `format_version = "0.2"` — the geometry-optional contract.
    V0_2,
}

impl FormatVersion {
    pub fn as_str(&self) -> &'static str {
        match self {
            FormatVersion::V0_1 => V0_1_STR,
            FormatVersion::V0_2 => V0_2_STR,
        }
    }

    /// Whether this version **expects** the `outlines.geoparquet` geometry rollup
    /// (the geometry-optional relaxation, spec §4/§9, Fusion Arc 0.2).
    ///
    /// HDX 0.1 mandates `outlines.geoparquet` for every dataset (§4 L1, §9 Geo1, §3 I1),
    /// so geometry is [`GeometryExpectation::Required`]. HDX 0.2 makes it
    /// [`GeometryExpectation::Optional`]: a pure-scalar dataset (`scalar_static` present,
    /// `outlines` absent) is conformant — the `validate` L1 outlines leg is skipped, not
    /// failed. The expectation is derived **only** from the version, so the predicate is
    /// resolved identically wherever the manifest is in hand.
    pub fn geometry_expectation(&self) -> GeometryExpectation {
        match self {
            FormatVersion::V0_1 => GeometryExpectation::Required,
            FormatVersion::V0_2 => GeometryExpectation::Optional,
        }
    }
}

/// Whether a dataset's geometry rollup (`outlines.geoparquet`) is required or optional —
/// the geometry-optional predicate the §14 L1 outlines leg branches on (spec §4/§9).
///
/// An enum, never a `bool`, so the geometry-optional decision is self-documenting at the
/// call site (architecture §3.3). Derived from the [`FormatVersion`] via
/// [`FormatVersion::geometry_expectation`]: 0.1 ⇒ [`Required`](GeometryExpectation::Required),
/// 0.2 ⇒ [`Optional`](GeometryExpectation::Optional).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeometryExpectation {
    /// `outlines.geoparquet` MUST be present (HDX 0.1) — an absent outlines is an L1 fail.
    Required,
    /// `outlines.geoparquet` is OPTIONAL (HDX 0.2) — an absent outlines skips the L1 leg.
    Optional,
}

impl GeometryExpectation {
    /// The geometry expectation a given [`FormatVersion`] carries (the inverse spelling of
    /// [`FormatVersion::geometry_expectation`], for call sites that hold the version).
    pub fn for_version(version: FormatVersion) -> Self {
        version.geometry_expectation()
    }
}

impl FromStr for FormatVersion {
    type Err = CoreError;

    /// Parses a raw `format_version` string under the hard version cut.
    ///
    /// The match is exact: only `"0.1"` and `"0.2"` are accepted (no numeric
    /// coercion, so `"0.10"`, `"0.1.0"`, and `"1.0"` all fail).
    ///
    /// # Errors
    ///
    /// | Condition | Error |
    /// |---|---|
    /// | `s` is any string other than `"0.1"` or `"0.2"` | [`CoreError::UnknownFormatVersion`] (with `found` echoing `s`) |
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            V0_1_STR => Ok(FormatVersion::V0_1),
            V0_2_STR => Ok(FormatVersion::V0_2),
            other => {
                warn!(found = other, "rejecting unknown format_version (hard cut)");
                Err(CoreError::UnknownFormatVersion {
                    found: other.to_string(),
                })
            }
        }
    }
}

impl TryFrom<&str> for FormatVersion {
    type Error = CoreError;

    /// Parses a raw `format_version` string; see [`FormatVersion::from_str`].
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::UnknownFormatVersion`] for any string but `"0.1"`
    /// or `"0.2"`.
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl fmt::Display for FormatVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use crate::error::CoreError;
    use crate::format_version::FormatVersion;

    #[test]
    fn parses_the_only_supported_version() {
        // `CoreError` is not `PartialEq`, so assert on the `Ok` arm directly
        // rather than comparing the whole `Result`.
        assert_eq!(
            FormatVersion::from_str("0.1").expect("\"0.1\" must parse to the sole arm"),
            FormatVersion::V0_1
        );
    }

    #[test]
    fn try_from_parses_the_only_supported_version() {
        assert_eq!(
            FormatVersion::try_from("0.1").expect("\"0.1\" must parse via TryFrom"),
            FormatVersion::V0_1
        );
    }

    #[test]
    fn parses_0_2_and_round_trips() {
        // The HDX 0.2 contract arm: `"0.2"` parses to `FormatVersion::V0_2` and
        // round-trips bit-for-bit through `as_str()` and `Display`.
        assert_eq!(
            FormatVersion::from_str("0.2").expect("\"0.2\" must parse to the V0_2 arm"),
            FormatVersion::V0_2
        );
        assert_eq!(FormatVersion::V0_2.as_str(), "0.2");
        assert_eq!(FormatVersion::V0_2.to_string(), "0.2");
        // Display output re-parses to the same value.
        assert_eq!(
            FormatVersion::from_str(&FormatVersion::V0_2.to_string())
                .expect("Display output must re-parse"),
            FormatVersion::V0_2
        );
    }

    #[test]
    fn rejects_every_other_string_with_echoed_input() {
        // Exact-string match, no numeric coercion: "0.10" != "0.1", etc.
        // ("0.2" is now an accepted contract version; see parses_0_2_and_round_trips.)
        for input in ["1.0", "", "0.1.0", "0.10"] {
            match FormatVersion::from_str(input) {
                Err(CoreError::UnknownFormatVersion { found }) => {
                    assert_eq!(found, input, "the error must echo the rejected input");
                }
                other => panic!("expected UnknownFormatVersion for {input:?}, got {other:?}"),
            }
        }
    }

    #[test]
    fn try_from_rejects_unknown_versions() {
        match FormatVersion::try_from("1.0") {
            Err(CoreError::UnknownFormatVersion { found }) => assert_eq!(found, "1.0"),
            other => panic!("expected UnknownFormatVersion, got {other:?}"),
        }
    }

    #[test]
    fn as_str_and_display_round_trip_to_canonical() {
        assert_eq!(FormatVersion::V0_1.as_str(), "0.1");
        assert_eq!(FormatVersion::V0_1.to_string(), "0.1");
        // Display output re-parses to the same value.
        assert_eq!(
            FormatVersion::from_str(&FormatVersion::V0_1.to_string())
                .expect("Display output must re-parse"),
            FormatVersion::V0_1
        );
    }
}
