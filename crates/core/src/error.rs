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
