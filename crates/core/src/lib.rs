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
//! - [`format_version`] — the single-arm [`FormatVersion`](format_version::FormatVersion)
//!   hard version cut.
//! - [`field`] — the field 2×2 quadrant model and the closed [`Dtype`](field::Dtype).
//! - [`grid`] — the shared gridded-geometry value types ([`GridExtent`](grid::GridExtent)
//!   with the single cell-edge convention + the Zarr center→edge half-pixel rule,
//!   [`GridResolution`](grid::GridResolution), [`GridInfo`](grid::GridInfo)) the
//!   gridded/geometry readers (MS4) consume; pure types, no IO.
//! - [`manifest`] — the six-field [`Manifest`](manifest::Manifest) boundary parse.
//! - [`layout`] — the basin-first hive walk into a typed
//!   [`LayoutModel`](layout::LayoutModel): root-rollup presence + enumerated
//!   `basin=<id>` dirs with per-basin artifact paths (filesystem-only, no reads).
//! - [`scalar_reader`] — the scalar-parquet metadata reader: arrow schema → MS1
//!   [`Field`](field::Field)s, `basin_id` presence + value, the `time` descriptor,
//!   and per-basin time extents (row-group statistics with a bounded 1-D fallback).
//! - [`discovery`] — the **scalar half** of the shared discovery layer: the typed
//!   [`ScalarDiscovery`](discovery::ScalarDiscovery) model and the single boundary
//!   function [`discover_scalar`](discovery::discover_scalar) that walks the tree,
//!   reads every scalar artifact, and returns the basin list + scalar field catalog +
//!   per-basin time descriptors/extents + folder-vs-in-file `basin_id` pairs +
//!   root-rollup presence facts both verbs consume (the gridded/geometry half is MS4).
//! - `parquet_meta` (private) — the crate's single touchpoint into the pure-Rust
//!   `parquet`/`arrow` stack (R1): opens a parquet byte source and recovers its
//!   metadata (arrow schema + row-group statistics) only — never a chunk. The scalar
//!   reader is layered on this metadata path.

pub mod discovery;
pub mod error;
pub mod field;
pub mod format_version;
pub mod grid;
pub mod layout;
pub mod manifest;
pub mod newtypes;
pub mod scalar_reader;

// The parquet metadata touchpoint (MS3-S1): the scalar reader is its first non-test
// consumer, so it is a live private module — no dead-code allow needed.
mod parquet_meta;

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
            CoreError::MismatchedGridLabel {
                field: "elevation".to_string(),
                gridded: true,
                has_label: false,
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
            CoreError::LayoutWalk {
                path: "/no/such/dir".to_string(),
                detail: "No such file or directory".to_string(),
            },
            CoreError::MissingScalarColumn {
                artifact: "scalar_dynamic.parquet".to_string(),
                column: "time".to_string(),
            },
            CoreError::ZarrRead {
                artifact: "era5.zarr".to_string(),
                detail: "malformed zarr.json".to_string(),
            },
            CoreError::CogRead {
                artifact: "era5.tif".to_string(),
                detail: "not a valid TIFF".to_string(),
            },
            CoreError::GeoparquetRead {
                artifact: "outlines.geoparquet".to_string(),
                detail: "malformed footer".to_string(),
            },
            CoreError::MissingGridGeoref {
                artifact: "era5.zarr".to_string(),
                detail: "no grid_mapping target".to_string(),
            },
            CoreError::MissingGriddedCoordinate {
                artifact: "era5.zarr".to_string(),
                coordinate: "lon".to_string(),
            },
            CoreError::MissingGeometryColumn {
                artifact: "outlines.geoparquet".to_string(),
                column: "delineation".to_string(),
            },
        ];

        // Every variant must render a non-empty Display string.
        for variant in &variants {
            assert!(!variant.to_string().is_empty());
        }
        assert_eq!(variants.len(), 21);
    }
}
