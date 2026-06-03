//! `hdx-core` — all contract logic for HDX v0.1 (Hydrology Dataset Exchange).
//!
//! HDX describes the *shape* of per-basin hydrology data, never *what was done to
//! it*: the crate is **inert and agnostic** (spec §1). No type or field here
//! carries transform, role, semantic type, or provenance. The spec and its
//! validator are the same artifact — the `validate` and `describe` boundary verbs
//! live in this crate, built on a parse-don't-validate type model whose raw input
//! is converted into valid-by-construction domain types at the boundary.
//!
//! Module map:
//!
//! - [`newtypes`] — opaque domain newtypes wrapping producer strings.
//! - [`error`] — the crate-wide [`CoreError`](error::CoreError) thiserror enum.
//! - [`format_version`] — the single-arm [`FormatVersion`](format_version::FormatVersion)
//!   hard version cut.
//! - [`field`] — the field 2×2 quadrant model and the closed [`Dtype`](field::Dtype).
//! - [`grid`] — the shared gridded-geometry value types ([`GridExtent`](grid::GridExtent)
//!   with the single cell-edge convention + the Zarr center→edge half-pixel rule,
//!   [`GridResolution`](grid::GridResolution), [`GridInfo`](grid::GridInfo)) the
//!   gridded/geometry readers consume; pure types, no IO.
//! - [`manifest`] — the six-field [`Manifest`](manifest::Manifest) boundary parse.
//! - [`layout`] — the basin-first hive walk into a typed
//!   [`LayoutModel`](layout::LayoutModel): root-rollup presence + enumerated
//!   `basin=<id>` dirs with per-basin artifact paths (filesystem-only, no reads).
//! - [`scalar_reader`] — the scalar-parquet metadata reader: arrow schema →
//!   [`Field`](field::Field)s, `basin_id` presence + value, the `time` descriptor,
//!   and per-basin time extents (row-group statistics with a bounded 1-D fallback).
//! - [`discovery`] — the **scalar half** of the shared discovery layer: the typed
//!   [`ScalarDiscovery`](discovery::ScalarDiscovery) model and the single boundary
//!   function [`discover_scalar`](discovery::discover_scalar) that walks the tree,
//!   reads every scalar artifact, and returns the basin list + scalar field catalog +
//!   per-basin time descriptors/extents + folder-vs-in-file `basin_id` pairs +
//!   root-rollup presence facts both verbs consume.
//! - `parquet_meta` (private) — the crate's single touchpoint into the pure-Rust
//!   `parquet`/`arrow` stack: opens a parquet byte source and recovers its
//!   metadata (arrow schema + row-group statistics) only — never a chunk. The scalar
//!   reader is layered on this metadata path.
//! - [`zarr_reader`] — the Zarr v3 **metadata** reader: reads a
//!   `gridded_dynamic/<label>.zarr` store via the §8 inline consolidated-metadata
//!   path (one read of the root `zarr.json`), classifies its arrays, reads the 1-D
//!   `lat`/`lon`/`time` coordinate chunks, and builds a [`GridInfo`](grid::GridInfo)
//!   with the center→edge conversion plus an ordinary `GriddedDynamic`
//!   [`Field`](field::Field) per data variable. Metadata + 1-D coordinate reads only
//!   — never a `c/0/0/0` data chunk.
//! - [`cog_reader`] — the COG / GeoTIFF **metadata** reader: reads a
//!   `gridded_static/<label>.tif` artifact **tags only** — the band description
//!   (= field name) + units from tag 42112 `GDAL_METADATA`, and the standard GeoTIFF
//!   georef tags (`ModelPixelScale`, `ModelTiepoint`, `ImageWidth`/`ImageLength`,
//!   `GeoKeyDirectory` EPSG) into an edge-based [`GridInfo`](grid::GridInfo) plus an
//!   ordinary `GriddedStatic` [`Field`](field::Field). Never decodes a pixel
//!   raster; the edge extent matches the Zarr reader's for an aligned grid.
//! - [`geoparquet_reader`] — the `outlines.geoparquet` **metadata + 1-D column**
//!   reader: reuses the same private `parquet`/`arrow` touchpoint to read the arrow
//!   schema (the `basin_id`/`delineation`/`geometry` presence check, spec §9 Geo1),
//!   a bounded 1-D read of the `delineation` labels + `basin_id` values (the
//!   `geometry` blob is never decoded), and the `geo` key-value PROJJSON CRS recorded
//!   as a comparable `EPSG:<code>` from its `id` (raw PROJJSON kept with a flag when
//!   no EPSG `id` resolves) so the §14 M5 CRS check receives a value comparable to
//!   the manifest's `"EPSG:4326"`.
//! - [`gridded_discovery`] — the **gridded / geometry half** of the shared discovery
//!   layer plus the **combined** model: the typed
//!   [`GriddedDiscovery`](gridded_discovery::GriddedDiscovery) model and its boundary
//!   function [`discover_gridded`](gridded_discovery::discover_gridded) that walk the
//!   tree, read every present COG / Zarr artifact + the outlines schema, and return
//!   the per-grid geometries + the gridded field catalog + the delineation labels +
//!   the per-basin observed grid labels (the §14 G2 precondition fact) + the Zarr
//!   path. The [`Discovery`](gridded_discovery::Discovery) struct **pairs** this with
//!   the [`ScalarDiscovery`](discovery::ScalarDiscovery) half without reshaping
//!   either, so both verbs consume one model;
//!   [`discover`](gridded_discovery::discover) builds both halves in one call. Records
//!   facts, never a verdict.
//! - [`describe`] — the `describe` self-description type ([`Description`](describe::Description)),
//!   its describe-local `#[derive(Serialize)]` DTO layer (the wire shape, spec §10),
//!   and the boundary verb [`describe`](describe::describe) /
//!   [`describe_json`](describe::describe_json) itself. The DTO owns the JSON shape in
//!   one place so the inert domain types stay free of `serde::Serialize`; the pure
//!   mapping `Discovery + Manifest → Description → DTO` reports **facts only — no
//!   conformance verdict**. The verb's entry order is **load-bearing** (spec §0): it
//!   (1) reads `manifest.json`, (2) hard-cuts `format_version` via
//!   [`Manifest::from_json`](manifest::Manifest::from_json) — returning on an unknown
//!   version **before** any other file is touched — (3) runs
//!   [`discover`](gridded_discovery::discover), then (4) assembles the
//!   [`Description`](describe::Description). Errors are the boundary
//!   [`DescribeError`](error::DescribeError) (it wraps [`CoreError`](error::CoreError)
//!   so the hard cut surfaces unchanged).
//! - [`validate`] — the `validate` conformance verb (spec §10/§14): runs the §14 `MUST`
//!   checklist over the same shared [`Discovery`](gridded_discovery::Discovery) model and
//!   emits a [`ValidationReport`](validate::ValidationReport) of per-check
//!   [`CheckOutcome`](validate::CheckOutcome)s (each recording **ran vs skipped**, a
//!   **pass/fail** result, and its [`DepthClass`](validate::DepthClass)) plus an
//!   overall `conformant: bool`. The verb's **entry order mirrors `describe`** (spec §0):
//!   (1) read `manifest.json`, (2) [`Manifest::from_json`](manifest::Manifest::from_json)
//!   hard-cut `format_version` — returning before discovery — (3)
//!   [`discover`](gridded_discovery::discover), then (4) run the §14 rules. The
//!   **report-vs-error split is load-bearing**: a violated `MUST` that ran is a recorded
//!   fail [`CheckOutcome`](validate::CheckOutcome) (so `conformant: false`), never a
//!   returned `Err`; a [`ValidateError`](error::ValidateError) is reserved for
//!   **structural / entry** failures (an unreadable manifest, the §0 hard cut, an
//!   undecodable present artifact) so the CLI can map the two to distinct exit codes.
//!   The report lists all 20 §14 ids: the in-memory checks (M1–M4 via the entry gate;
//!   H1, H2, I3, T1, G1) and the cross-file checks (L1, L2, I1, I2, M5, G2, G3) `ran`
//!   (pass/fail), while the byte-deep / on-disk-shape-dependent legs (L3, M6 rule (b),
//!   T2, Geo1-when-outlines-absent) are honest `Skipped`-with-reason. The report's
//!   **JSON wire shape** is pinned by a validate-local `#[derive(Serialize)]`
//!   [`ValidationReportDto`](validate::ValidationReportDto) (the inert types stay
//!   serde-free, mirroring `describe`), [`validate_json`](validate::validate_json), and
//!   a committed golden report checked against `schemas/validate.schema.json` — making
//!   the §14-note "report which checks ran" requirement a machine-readable, pinned
//!   artifact.

pub mod cog_reader;
pub mod describe;
pub mod discovery;
pub mod error;
pub mod field;
pub mod format_version;
pub mod geoparquet_reader;
pub mod grid;
pub mod gridded_discovery;
pub mod layout;
pub mod manifest;
pub mod newtypes;
pub mod scalar_reader;
pub mod validate;
pub mod zarr_reader;

// The parquet metadata touchpoint; the scalar reader is its first consumer.
mod parquet_meta;

#[cfg(test)]
mod tests {
    use crate::error::CoreError;

    /// Constructs every [`CoreError`] variant so the whole error surface is
    /// exercised and every variant stays referenced.
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
