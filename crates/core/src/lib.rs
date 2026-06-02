//! `hdx-core` — all contract logic for HDX v0.1 (Hydrology Dataset Exchange).
//!
//! HDX describes the *shape* of per-basin hydrology data, never *what was done to
//! it*: the crate is **inert and agnostic** (spec §1). No type or field here
//! carries transform, role, semantic type, or provenance. The spec and its
//! validator are the same artifact — `validate` and `describe` (later milestones)
//! live in this crate, built on a parse-don't-validate type model whose raw input
//! is converted into valid-by-construction domain types at the boundary.
//!
//! Module map (modules are added as their milestone steps land):
//!
//! - [`newtypes`] — opaque domain newtypes wrapping producer strings.
//! - [`error`] — the crate-wide [`CoreError`](error::CoreError) thiserror enum.

pub mod error;
pub mod newtypes;

#[cfg(test)]
mod tests {
    use crate::error::CoreError;

    /// Constructs every [`CoreError`] variant so the error surface is exercised
    /// and the later-milestone skeleton variants are referenced (documents intent
    /// and keeps clippy quiet on the reserved variants).
    #[test]
    fn every_core_error_variant_constructs() {
        let variants = [
            CoreError::UnknownFormatVersion {
                found: "0.2".to_string(),
            },
            CoreError::ExtraManifestField {
                field: "content_hash".to_string(),
            },
            CoreError::MissingManifestField {
                field: "cadence".to_string(),
            },
            CoreError::InvalidTimestamp {
                value: "not-a-date".to_string(),
            },
            CoreError::EmptyCrs,
            CoreError::EmptyCadence,
            CoreError::UnknownDtype {
                found: "complex128".to_string(),
            },
            CoreError::BasinIdFolderMismatch {
                in_file: "a".to_string(),
                folder: "b".to_string(),
            },
            CoreError::RaggedSchema {
                basin: "a".to_string(),
            },
            CoreError::GridLabelMismatchAcrossBasins {
                label: "era5".to_string(),
            },
            CoreError::MissingRootRollup {
                artifact: "scalar_static.parquet".to_string(),
            },
            CoreError::NonMonotonicTime {
                artifact: "scalar_dynamic.parquet".to_string(),
            },
        ];

        // Every variant must render a non-empty Display string.
        for variant in &variants {
            assert!(!variant.to_string().is_empty());
        }
        assert_eq!(variants.len(), 12);
    }
}
