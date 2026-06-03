//! The one contract-version axis for HDX, encoded as a hard version cut.
//!
//! [`FormatVersion`] has exactly one arm, [`FormatVersion::V0_1`]. Parsing a raw
//! `format_version` string succeeds **only** on `"0.1"` and errors on anything
//! else (spec §0/§14 M2): the version is a hard cut, read before any other field
//! and rejected outright when unknown. There are **no multi-version readers** —
//! that state is not representable, because the enum has no other arm to land in.
//!
//! The parse is exact-string (`"0.10"` is not `"0.1"`); HDX performs no numeric
//! coercion.

use std::fmt;
use std::str::FromStr;

use tracing::warn;

use crate::error::CoreError;

/// The canonical `format_version` string this build implements.
const V0_1_STR: &str = "0.1";

/// The only contract-version axis HDX recognizes (spec §0/§11).
///
/// The single arm makes the hard cut a type-level invariant: a value of this
/// type is, by construction, the one version this build implements. Parsing is
/// the only way to obtain one, and parsing rejects every string but `"0.1"`, so
/// no multi-version reader can exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatVersion {
    /// HDX `format_version = "0.1"` — the sole version defined by this spec.
    V0_1,
}

impl FormatVersion {
    pub fn as_str(&self) -> &'static str {
        match self {
            FormatVersion::V0_1 => V0_1_STR,
        }
    }
}

impl FromStr for FormatVersion {
    type Err = CoreError;

    /// Parses a raw `format_version` string under the hard version cut.
    ///
    /// The match is exact: only `"0.1"` is accepted (no numeric coercion, so
    /// `"0.10"`, `"0.1.0"`, and `"1.0"` all fail).
    ///
    /// # Errors
    ///
    /// | Condition | Error |
    /// |---|---|
    /// | `s` is any string other than `"0.1"` | [`CoreError::UnknownFormatVersion`] (with `found` echoing `s`) |
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            V0_1_STR => Ok(FormatVersion::V0_1),
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
    /// Returns [`CoreError::UnknownFormatVersion`] for any string but `"0.1"`.
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
    fn rejects_every_other_string_with_echoed_input() {
        // Exact-string match, no numeric coercion: "0.10" != "0.1", etc.
        for input in ["0.2", "1.0", "", "0.1.0", "0.10"] {
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
        match FormatVersion::try_from("0.2") {
            Err(CoreError::UnknownFormatVersion { found }) => assert_eq!(found, "0.2"),
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
