//! The crate-wide error surface for `hdx-core`.
//!
//! Every fallible path in the contract logic returns [`CoreError`]. Variants use
//! named fields (never tuples) and each is doc-commented with *when* it fires, so
//! a reader can map an error back to the spec rule that produced it. Several
//! variants are intentional skeletons for later milestones: they are listed here
//! up front so the error surface is stable and later steps slot in without
//! reshaping the enum.

/// Errors produced by `hdx-core` contract logic.
///
/// Library code never panics; every recoverable failure is one of these variants.
/// Variants are grouped by the milestone that first fires them. Variants marked as
/// reserved are wired in by later milestones; they are declared now to keep the
/// error surface stable across steps.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CoreError {
    /// Fires when `manifest.format_version` is any string other than the single
    /// supported version `"0.1"`. The version is a hard cut (spec §0/§14 M2): an
    /// unknown value is rejected outright before any other field is interpreted.
    #[error("unknown format_version {found:?}: only \"0.1\" is supported")]
    UnknownFormatVersion {
        /// The raw `format_version` string read from the manifest.
        found: String,
    },

    /// Fires when the manifest carries a field beyond the six floor fields
    /// (spec §11/§14 M3). The manifest is exactly six fields; any extra
    /// (derivable) field is a conformance bug.
    #[error("unexpected manifest field {field:?}: the manifest floor is exactly six fields")]
    ExtraManifestField {
        /// The name of the offending extra field.
        field: String,
    },

    /// Fires when a required floor field is absent from the manifest
    /// (spec §11/§14 M3). All six floor fields must be present.
    #[error("missing manifest field {field:?}")]
    MissingManifestField {
        /// The name of the required field that was missing.
        field: String,
    },

    /// Fires when `manifest.created_at` is not a valid RFC 3339 timestamp
    /// (spec §11/§14 M4).
    #[error("invalid RFC 3339 timestamp {value:?}")]
    InvalidTimestamp {
        /// The raw timestamp string that failed to parse.
        value: String,
    },

    /// Fires when `manifest.crs` is an empty string (spec §11/§14 M4); the CRS
    /// must be a non-empty string.
    #[error("crs must be a non-empty string")]
    EmptyCrs,

    /// Fires when `manifest.cadence` is an empty string (spec §11/§14 M4); the
    /// cadence must be a non-empty string.
    #[error("cadence must be a non-empty string")]
    EmptyCadence,

    /// Fires when a declared dtype string does not map to a supported [`Dtype`]
    /// (spec §2). HDX rejects unknown dtypes rather than carrying them, so the
    /// dtype set stays closed and semantics-opaque.
    ///
    /// [`Dtype`]: crate::field::Dtype
    #[error("unknown dtype {found:?}")]
    UnknownDtype {
        /// The raw dtype string that did not map to a supported variant.
        found: String,
    },

    /// Fires when [`Field::new`] is handed a `grid_label` that disagrees with the
    /// field's shape axis: a `gridded` field MUST carry a grid label and a
    /// `scalar` field MUST NOT (spec §2/§8, architecture §3.3). This keeps the
    /// `grid_label.is_some()` ⇔ `Shape::Gridded` invariant a construction-time
    /// guarantee rather than a runtime hazard.
    ///
    /// [`Field::new`]: crate::field::Field::new
    #[error(
        "field {field:?}: grid_label presence ({has_label}) does not match shape (gridded={gridded})"
    )]
    MismatchedGridLabel {
        /// The name of the field whose shape and grid label disagreed.
        field: String,
        /// `true` if the field's shape axis is `gridded` (so a label is required).
        gridded: bool,
        /// `true` if a grid label was supplied.
        has_label: bool,
    },

    /// Fires when a parquet artifact cannot be opened or its metadata fails to
    /// decode (spec §4/§8, architecture §1): the byte source is not a valid parquet
    /// file, its footer/metadata is malformed, or the arrow schema cannot be read.
    /// MS3's scalar reader uses this as its typed surface so a corrupt or
    /// non-parquet input is reported, never panicked over. The variant stays
    /// **inert/agnostic**: it carries only the artifact name and an opaque detail
    /// string from the underlying reader — no domain field, no provenance.
    #[error("failed to read parquet metadata for {artifact:?}: {detail}")]
    ParquetRead {
        /// A name for the artifact that failed (a path or `"<in-memory>"`); used
        /// only for the diagnostic message, not interpreted.
        artifact: String,
        /// The underlying reader's error rendered as a string; opaque to HDX.
        detail: String,
    },

    /// Fires when the dataset path handed to the layout walk is not a readable
    /// directory: it does not exist, is a file rather than a directory, or its
    /// entries cannot be listed (a permissions/IO failure). The walk reports this
    /// typed error instead of panicking; it is a structural failure of the walk
    /// itself, distinct from the *facts* the walk records (a missing root rollup is
    /// recorded as absent, never raised — L1 enforcement is MS6). The variant stays
    /// **inert/agnostic** (spec §1): it carries only the offending path and an
    /// opaque detail string from the underlying filesystem error — no domain field.
    #[error("failed to walk dataset layout at {path:?}: {detail}")]
    LayoutWalk {
        /// The dataset path that could not be walked (used only for the diagnostic
        /// message, never interpreted).
        path: String,
        /// The underlying filesystem error rendered as a string; opaque to HDX.
        detail: String,
    },

    /// Fires when the scalar reader cannot find a column the parquet artifact is
    /// *structurally* required to carry: the `time` column in a per-basin
    /// `scalar_dynamic.parquet` (spec §6), or the `basin_id` column where a read of
    /// its value is requested (spec §3). This is a schema-level absence detected
    /// from the arrow schema — distinct from [`CoreError::ParquetRead`], which fires
    /// when the file or its metadata cannot be decoded at all. The reader surfaces
    /// the typed error instead of panicking. The variant stays **inert/agnostic**
    /// (spec §1): it carries only the artifact name and the missing column name —
    /// no domain field, no provenance.
    #[error("scalar artifact {artifact:?} is missing required column {column:?}")]
    MissingScalarColumn {
        /// A name for the artifact that lacked the column (a path or in-memory
        /// label); used only for the diagnostic message, never interpreted.
        artifact: String,
        /// The name of the structurally required column that was absent.
        column: String,
    },

    /// Fires when a Zarr v3 store cannot be opened or its metadata fails to decode
    /// (spec §7/§8, architecture §1): the store's root `zarr.json` is missing or
    /// malformed, its consolidated metadata cannot be parsed, or a 1-D coordinate
    /// array's metadata is unreadable. MS4's Zarr reader uses this as its typed
    /// surface so a corrupt or non-Zarr input is reported, never panicked over. The
    /// reader reads metadata + 1-D coordinate arrays only — never a data chunk. The
    /// variant stays **inert/agnostic** (spec §1): it carries only the artifact name
    /// and an opaque detail string from the underlying reader — no domain field.
    #[error("failed to read Zarr metadata for {artifact:?}: {detail}")]
    ZarrRead {
        /// A name for the store that failed (a path or in-memory label); used only
        /// for the diagnostic message, never interpreted.
        artifact: String,
        /// The underlying reader's error rendered as a string; opaque to HDX.
        detail: String,
    },

    /// Fires when a COG / GeoTIFF artifact cannot be opened or its tags fail to
    /// decode (spec §7/§8, architecture §1): the byte source is not a valid TIFF,
    /// its IFD is malformed, or a required tag cannot be read. MS4's COG reader uses
    /// this as its typed surface so a corrupt or non-TIFF input is reported, never
    /// panicked over. The reader reads tags + band metadata + georef only — never a
    /// pixel raster. The variant stays **inert/agnostic** (spec §1): it carries only
    /// the artifact name and an opaque detail string from the reader — no domain field.
    #[error("failed to read COG tags for {artifact:?}: {detail}")]
    CogRead {
        /// A name for the artifact that failed (a path or in-memory label); used
        /// only for the diagnostic message, never interpreted.
        artifact: String,
        /// The underlying reader's error rendered as a string; opaque to HDX.
        detail: String,
    },

    /// Fires when the `outlines.geoparquet` artifact cannot be opened or its
    /// metadata fails to decode (spec §9, architecture §1): the byte source is not a
    /// valid parquet file, its footer/`geo` key-value metadata is malformed, or the
    /// arrow schema cannot be read. MS4's geoparquet reader uses this as its typed
    /// surface so a corrupt input is reported, never panicked over. The reader reads
    /// the schema + the 1-D `basin_id`/`delineation` columns + the `geo` KV only —
    /// never the `geometry` blob. The variant stays **inert/agnostic** (spec §1): it
    /// carries only the artifact name and an opaque detail string — no domain field.
    #[error("failed to read geoparquet metadata for {artifact:?}: {detail}")]
    GeoparquetRead {
        /// A name for the artifact that failed (a path or in-memory label); used
        /// only for the diagnostic message, never interpreted.
        artifact: String,
        /// The underlying reader's error rendered as a string; opaque to HDX.
        detail: String,
    },

    /// Fires when a gridded artifact carries no resolvable georeferencing
    /// (spec §7.3, feeds §14 G3): a Zarr data variable has no CF `grid_mapping`
    /// target, or a GeoTIFF has no standard georef tags (no `ModelTiepoint` /
    /// `ModelPixelScale` / `GeoKeyDirectory`). The georef is structurally required
    /// to place the grid, so its absence is reported as a typed error rather than a
    /// fabricated extent. The variant stays **inert/agnostic** (spec §1): it carries
    /// only the artifact name and an opaque detail string naming what was missing.
    #[error("gridded artifact {artifact:?} is missing georeferencing: {detail}")]
    MissingGridGeoref {
        /// A name for the artifact that lacked georeferencing (a path or in-memory
        /// label); used only for the diagnostic message, never interpreted.
        artifact: String,
        /// An opaque description of which georef facet was absent (e.g. the missing
        /// CF `grid_mapping` target or GeoTIFF tag); not interpreted by HDX.
        detail: String,
    },

    /// Fires when a required 1-D coordinate array is absent from a Zarr store
    /// (spec §7.3): the `lat`, `lon`, or `time` coordinate array the CF convention
    /// mandates cannot be found. This is a structural absence detected from the
    /// store metadata — distinct from [`CoreError::ZarrRead`], which fires when the
    /// store or its metadata cannot be decoded at all. The variant stays
    /// **inert/agnostic** (spec §1): it carries only the artifact name and the
    /// missing coordinate name — no domain field, no provenance.
    #[error("Zarr store {artifact:?} is missing required coordinate array {coordinate:?}")]
    MissingGriddedCoordinate {
        /// A name for the store that lacked the coordinate (a path or in-memory
        /// label); used only for the diagnostic message, never interpreted.
        artifact: String,
        /// The name of the structurally required coordinate array that was absent
        /// (`lat`, `lon`, or `time`).
        coordinate: String,
    },

    /// Fires when `outlines.geoparquet` lacks one of the three columns its schema is
    /// structurally required to carry (spec §9, feeds §14 Geo1): `basin_id`,
    /// `delineation`, or `geometry`. This is a schema-level absence detected from the
    /// arrow schema — distinct from [`CoreError::GeoparquetRead`], which fires when
    /// the file or its metadata cannot be decoded at all. The variant stays
    /// **inert/agnostic** (spec §1): it carries only the artifact name and the
    /// missing column name — no domain field, no provenance.
    #[error("outlines artifact {artifact:?} is missing required column {column:?}")]
    MissingGeometryColumn {
        /// A name for the artifact that lacked the column (a path or in-memory
        /// label); used only for the diagnostic message, never interpreted.
        artifact: String,
        /// The name of the structurally required column that was absent
        /// (`basin_id`, `delineation`, or `geometry`).
        column: String,
    },

    // --- Reserved for later milestones (skeleton variants) ---
    /// Reserved for MS6: fires when an in-file `basin_id` disagrees with its
    /// `basin=<id>` partition folder (spec §3/§14 I2).
    #[error("basin_id {in_file:?} does not match its partition folder {folder:?}")]
    BasinIdFolderMismatch {
        /// The `basin_id` value read from inside the file.
        in_file: String,
        /// The id parsed from the `basin=<id>` folder name.
        folder: String,
    },

    /// Reserved for MS6: fires when a basin's field schema differs from the
    /// dataset's homogeneous schema (spec §5/§14 H1).
    #[error("ragged schema: basin {basin:?} does not share the dataset field schema")]
    RaggedSchema {
        /// The `basin_id` whose schema diverged.
        basin: String,
    },

    /// Reserved for MS6: fires when the set of grid labels differs across basins
    /// (spec §8/§14 H2).
    #[error("grid-label set differs across basins for label {label:?}")]
    GridLabelMismatchAcrossBasins {
        /// The grid label that is not present uniformly across basins.
        label: String,
    },

    /// Reserved for MS6: fires when a required root rollup
    /// (`scalar_static.parquet` or `outlines.geoparquet`) is absent
    /// (spec §4/§14 L1).
    #[error("missing root rollup {artifact:?}")]
    MissingRootRollup {
        /// The name of the missing root artifact.
        artifact: String,
    },

    /// Reserved for MS6: fires when a `time` axis is not sorted strictly
    /// ascending (spec §6/§14 T1).
    #[error("non-monotonic time axis in {artifact:?}")]
    NonMonotonicTime {
        /// The artifact whose `time` axis was not monotonically increasing.
        artifact: String,
    },
}
