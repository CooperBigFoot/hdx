//! The crate-wide error surface for `hdx-core`.
//!
//! Every fallible path in the contract logic returns [`CoreError`]. Variants use
//! named fields (never tuples) and each is doc-commented with *when* it fires, so
//! a reader can map an error back to the spec rule that produced it.

/// Errors produced by `hdx-core` contract logic.
///
/// Library code never panics; every recoverable failure is one of these variants.
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
    /// The scalar reader uses this as its typed surface so a corrupt or non-parquet
    /// input is reported, never panicked over. The variant stays **inert/agnostic**:
    /// it carries only the artifact name and an opaque detail string from the
    /// underlying reader — no domain field, no provenance.
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
    /// recorded as absent, never raised — spec §4/§14 L1). The variant stays
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
    /// array's metadata is unreadable. The Zarr reader uses this as its typed
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
    /// its IFD is malformed, or a required tag cannot be read. The COG reader uses
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
    /// arrow schema cannot be read. The geoparquet reader uses this as its typed
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

    /// Fires when an in-file `basin_id` disagrees with its `basin=<id>` partition
    /// folder (spec §3/§14 I2).
    #[error("basin_id {in_file:?} does not match its partition folder {folder:?}")]
    BasinIdFolderMismatch {
        /// The `basin_id` value read from inside the file.
        in_file: String,
        /// The id parsed from the `basin=<id>` folder name.
        folder: String,
    },

    /// Fires when a basin's field schema differs from the dataset's homogeneous
    /// schema (spec §5/§14 H1).
    #[error("ragged schema: basin {basin:?} does not share the dataset field schema")]
    RaggedSchema {
        /// The `basin_id` whose schema diverged.
        basin: String,
    },

    /// Fires when the set of grid labels differs across basins (spec §8/§14 H2).
    #[error("grid-label set differs across basins for label {label:?}")]
    GridLabelMismatchAcrossBasins {
        /// The grid label that is not present uniformly across basins.
        label: String,
    },

    /// Fires when a required root rollup (`scalar_static.parquet` or
    /// `outlines.geoparquet`) is absent (spec §4/§14 L1).
    #[error("missing root rollup {artifact:?}")]
    MissingRootRollup {
        /// The name of the missing root artifact.
        artifact: String,
    },

    /// Fires when a `time` axis is not sorted strictly ascending (spec §6/§14 T1).
    #[error("non-monotonic time axis in {artifact:?}")]
    NonMonotonicTime {
        /// The artifact whose `time` axis was not monotonically increasing.
        artifact: String,
    },
}

/// Errors produced by the `describe` boundary verb
/// ([`describe`](crate::describe::describe), spec §10, architecture §5).
///
/// `describe` is the dataset's entry point, so its error surface is **distinct from**
/// [`CoreError`]: it adds one boundary-IO concern of its own ([`ManifestUnreadable`],
/// the `manifest.json` file is absent/unreadable) and otherwise **wraps `CoreError`
/// unchanged**, so the §0 hard-cut [`CoreError::UnknownFormatVersion`] and every
/// discovery error surface verbatim through this enum. The wrapping is deliberate (a
/// thin newtype-style enum over `CoreError`): the
/// caller can match on `Manifest(UnknownFormatVersion { .. })` to observe the hard cut
/// without `describe` having to re-export or re-interpret the inner variant. Library
/// code never panics; every recoverable failure is one of these variants.
///
/// [`ManifestUnreadable`]: DescribeError::ManifestUnreadable
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum DescribeError {
    /// Fires when `<dataset>/manifest.json` itself cannot be read as a file: it is
    /// absent, is a directory, or the read fails with a filesystem/permissions error
    /// (spec §0 — the manifest is read **first**, before any other file). This is
    /// **distinct from a malformed manifest**: a file that *is* read but whose
    /// contents are not a conformant six-field manifest surfaces through
    /// [`DescribeError::Manifest`] instead. The variant stays **inert/agnostic**
    /// (spec §1): it carries only the offending path and an opaque detail string from
    /// the underlying filesystem error — no domain field, no provenance.
    #[error("failed to read manifest.json at {path:?}: {detail}")]
    ManifestUnreadable {
        /// The `manifest.json` path that could not be read (used only for the
        /// diagnostic message, never interpreted).
        path: String,
        /// The underlying filesystem error rendered as a string; opaque to HDX.
        detail: String,
    },

    /// Fires when the `manifest.json` file was read but its contents are not a
    /// conformant six-field manifest — most importantly the §0/§14 M2 **hard version
    /// cut** ([`CoreError::UnknownFormatVersion`]), which `describe` evaluates
    /// **before** any discovery (spec §0 entry discipline). Wraps the inner
    /// [`CoreError`] unchanged so the boundary-parse failure surfaces verbatim (a
    /// caller can match `Manifest(CoreError::UnknownFormatVersion { .. })`).
    #[error("manifest boundary-parse failed: {0}")]
    Manifest(#[source] CoreError),

    /// Fires when discovery (the layout walk + the scalar/gridded metadata readers)
    /// fails **after** the manifest has been read and the hard version cut has
    /// passed. Wraps the inner [`CoreError`] unchanged so the underlying structural
    /// failure (e.g. [`CoreError::LayoutWalk`], [`CoreError::ZarrRead`]) surfaces
    /// verbatim. A discovery *gap* (a missing root rollup, a basin with no time
    /// extent) is **not** this error — gaps are recorded as facts in the
    /// [`Description`](crate::describe::Description), never raised (spec §10).
    #[error("discovery failed: {0}")]
    Discovery(#[source] CoreError),

    /// Fires when an assembled [`Description`](crate::describe::Description) cannot be
    /// serialized to the R4 JSON wire shape. In practice this branch is unreachable:
    /// the only fallible facet of serialization is the strict RFC 3339 formatting of a
    /// timestamp, which always succeeds for any `created_at` the manifest parser
    /// accepted. It is surfaced as a typed boundary error (never an `unwrap`/panic) so
    /// the no-panic guarantee holds even on a hypothetical serializer fault. The
    /// variant stays **inert/agnostic** (spec §1): it carries only an opaque detail
    /// string from `serde_json` — no domain field.
    #[error("failed to serialize the describe output: {detail}")]
    Serialize {
        /// The underlying `serde_json` error rendered as a string; opaque to HDX.
        detail: String,
    },
}

/// Errors produced by the `validate` boundary verb
/// ([`validate`](crate::validate::validate), spec §10/§14, architecture §5).
///
/// `validate` shares `describe`'s **§0 entry discipline**: it reads `manifest.json`
/// first and hard-cuts `format_version` **before** any discovery. Its error surface is
/// therefore shaped like [`DescribeError`] — one boundary-IO concern of its own
/// ([`ManifestUnreadable`]) plus thin wrappers over [`CoreError`] for the §0 hard cut /
/// malformed manifest ([`Manifest`]) and structural discovery faults ([`Discovery`]).
///
/// **The report-vs-error split is load-bearing.** A **violated
/// `MUST` that ran** is *never* a `ValidateError`: it is a recorded
/// [`CheckOutcome`](crate::validate::CheckOutcome) with
/// [`CheckResult::Fail`](crate::validate::CheckResult::Fail), which makes the
/// [`ValidationReport`](crate::validate::ValidationReport) non-`conformant`. A
/// `ValidateError` is only for **structural / entry** failures — an unreadable manifest,
/// the §0 hard version cut, an undecodable present artifact — so the CLI can map a
/// `ValidateError` to a distinct exit code from a `conformant: false` verdict. Library
/// code never panics; every recoverable failure is one of these variants.
///
/// [`ManifestUnreadable`]: ValidateError::ManifestUnreadable
/// [`Manifest`]: ValidateError::Manifest
/// [`Discovery`]: ValidateError::Discovery
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ValidateError {
    /// Fires when `<dataset>/manifest.json` itself cannot be read as a file: it is
    /// absent, is a directory, or the read fails with a filesystem/permissions error
    /// (spec §0 — the manifest is read **first**, before any other file). This is
    /// **distinct from a malformed manifest**: a file that *is* read but whose contents
    /// are not a conformant six-field manifest surfaces through
    /// [`ValidateError::Manifest`] instead. The variant stays **inert/agnostic**
    /// (spec §1): it carries only the offending path and an opaque detail string from
    /// the underlying filesystem error — no domain field, no provenance.
    #[error("failed to read manifest.json at {path:?}: {detail}")]
    ManifestUnreadable {
        /// The `manifest.json` path that could not be read (used only for the
        /// diagnostic message, never interpreted).
        path: String,
        /// The underlying filesystem error rendered as a string; opaque to HDX.
        detail: String,
    },

    /// Fires when the `manifest.json` file was read but its contents are not a
    /// conformant six-field manifest — most importantly the §0/§14 M2 **hard version
    /// cut** ([`CoreError::UnknownFormatVersion`]), which `validate` evaluates
    /// **before** any discovery (spec §0 entry discipline). Also wraps the M3/M4
    /// boundary failures ([`CoreError::ExtraManifestField`],
    /// [`CoreError::MissingManifestField`], [`CoreError::InvalidTimestamp`],
    /// [`CoreError::EmptyCrs`], [`CoreError::EmptyCadence`]) — the manifest parser
    /// rejects all of these at the boundary, so M3/M4 are recorded as `ran:pass` once
    /// the manifest parses and surface here as an `Err` when it does not. Wraps the
    /// inner [`CoreError`] unchanged so a caller can match
    /// `Manifest(CoreError::UnknownFormatVersion { .. })`.
    #[error("manifest boundary-parse failed: {0}")]
    Manifest(#[source] CoreError),

    /// Fires when discovery (the layout walk + the scalar/gridded metadata readers)
    /// fails **after** the manifest has been read and the hard version cut has passed.
    /// Wraps the inner [`CoreError`] unchanged so the underlying structural failure
    /// (e.g. [`CoreError::LayoutWalk`], [`CoreError::ZarrRead`]) surfaces verbatim. A
    /// **violated `MUST`** is *not* this error — it is a recorded fail
    /// [`CheckOutcome`](crate::validate::CheckOutcome), never raised (spec §10/§14).
    #[error("discovery failed: {0}")]
    Discovery(#[source] CoreError),

    /// Fires when an assembled
    /// [`ValidationReport`](crate::validate::ValidationReport) cannot be serialized to
    /// its JSON wire shape. Like [`DescribeError::Serialize`], this branch is
    /// effectively unreachable (the report carries only stable enum strings, ids, and
    /// opaque detail strings) but is surfaced as a typed boundary error so the no-panic
    /// guarantee holds. The variant stays **inert/agnostic** (spec §1): it carries only
    /// an opaque detail string from `serde_json` — no domain field.
    #[error("failed to serialize the validate report: {detail}")]
    Serialize {
        /// The underlying `serde_json` error rendered as a string; opaque to HDX.
        detail: String,
    },
}
