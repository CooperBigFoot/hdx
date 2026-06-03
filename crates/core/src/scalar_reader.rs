//! The scalar-parquet **metadata** reader (spec §2/§3/§6/§8, architecture §1).
//!
//! This module reads the *shape* of the two scalar artifacts — the dataset-level
//! `scalar_static.parquet` rollup and each per-basin `basin=<id>/scalar_dynamic.parquet`
//! — into typed facts, **never** their scientific values:
//!
//! - the **scalar field catalog**: the arrow schema mapped to [`Field`]s, one per
//!   data column, with the right scalar [`Quadrant`] ([`Quadrant::ScalarStatic`] for
//!   the static rollup, [`Quadrant::ScalarDynamic`] for the per-basin dynamic table)
//!   and a [`Dtype`] via [`parse_dtype`] over the arrow physical type;
//! - the **`basin_id` column**: its presence, and — for the dynamic table — the
//!   distinct in-file id value(s) (spec §3: the *authoritative* id; the folder id is
//!   only locality, paired with this for the I2 cross-check, never compared here);
//! - the **`time` column descriptor** ([`TimeColumn`]): name, logical [`Dtype`],
//!   nullability, and whether it is sorted ascending (spec §6 — `time` is a full
//!   timestamp, non-nullable, sorted ascending; T1 is enforced elsewhere);
//! - the **per-basin time extent** ([`TimeExtent`]): `[start, end]` with the
//!   [`TimeExtentSource`] that produced it.
//!
//! Fields are catalogued **purely by physical schema** with no name-pattern
//! special-casing: a column named `streamflow_was_filled` is an ordinary
//! `ScalarDynamic` field, not a companion mask (spec §1/§2).
//!
//! ## The time extent: statistics primary, a bounded 1-D fallback (spec §8)
//!
//! [`time_extent`] computes a basin's `[start, end]` two ways, recording which ran:
//!
//! - **[`TimeExtentSource::Statistics`] (primary, spec §8).** The `time` column's
//!   **row-group min/max statistics** give the extent directly from the footer — no
//!   data is read. `scalar_dynamic.parquet` is written sorted by `time` with
//!   row-group statistics (spec §8), so this is the live path on a conformant
//!   dataset.
//! - **[`TimeExtentSource::BoundedColumnScan`] (bounded fallback).** When the `time`
//!   column carries **no** row-group statistics, the reader falls back to a
//!   **bounded 1-D column read**: it projects **only the `time` column by name**
//!   (never `streamflow`, `drainage_area`, or any other data column, and never a
//!   gridded chunk) and recovers `[min, max]` from those timestamps. This is an
//!   architecture-§1-compliant metadata/index-tier read — a 1-D coordinate/key-column
//!   read, **not** a gridded-chunk value decode. The bound is hard: **exactly one
//!   column, selected by name.**
//!
//! ## Statistics hand-off rule
//!
//! The valid fixture's `time` column is written with usable row-group min/max
//! statistics, and a test asserts the `parquet` crate can read them back and that
//! [`time_extent`] sources its extent from [`TimeExtentSource::Statistics`] (not the
//! fallback). **The hand-off rule:** if the `parquet` crate ever *cannot* surface the
//! statistics the writer wrote, the fix is to **regenerate the fixture**, **never** a
//! reader workaround that papers over a writer/reader mismatch — treat a mismatch as
//! a generator bug.
//!
//! Like the layout walk, this reader is a discovery surface: it records facts and
//! enforces no spec §14 check. Per-basin extents differing across basins (§6.1 ragged
//! extents) are surfaced as facts, never raised.
//!
//! ## Glossary
//!
//! | Term | Meaning |
//! |---|---|
//! | field catalog | the per-artifact list of ordinary [`Field`]s from the arrow schema (spec §2) |
//! | `basin_id` | the authoritative in-file basin id column (spec §3) |
//! | `time` descriptor | the `time` column's name / dtype / nullability / sort facts (spec §6) |
//! | time extent | a basin's `[start, end]` timestamp pair (spec §6.1) |
//! | extent source | which path produced an extent: [`TimeExtentSource::Statistics`] vs [`TimeExtentSource::BoundedColumnScan`] |

use std::fs;
use std::path::Path;

use arrow::array::{
    Array, TimestampMicrosecondArray, TimestampMillisecondArray, TimestampNanosecondArray,
    TimestampSecondArray,
};
use arrow::datatypes::{DataType, Schema, TimeUnit};
use bytes::Bytes;
use parquet::arrow::ProjectionMask;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use parquet::file::metadata::ParquetMetaData;
use parquet::file::statistics::Statistics;
use time::OffsetDateTime;
use tracing::{debug, info, instrument, warn};

use crate::error::CoreError;
use crate::field::{Dtype, Field, Quadrant, Units, parse_dtype};
use crate::newtypes::{BasinId, FieldName};
use crate::parquet_meta::read_parquet_meta;

/// The name of the authoritative in-file basin-id column (spec §3).
const BASIN_ID_COLUMN: &str = "basin_id";
/// The name of the scalar `time` column (spec §6).
const TIME_COLUMN: &str = "time";

/// A single timestamp on the scalar `time` axis, normalized to UTC (spec §6).
///
/// HDX records the physical timestamp value and interprets nothing: the parquet
/// `Timestamp` element is a tick count in some [`TimeUnit`]; this newtype converts
/// it to a UTC [`OffsetDateTime`] so two extents are directly comparable regardless
/// of the on-disk unit. A naive (timezone-less) timestamp is read as UTC — HDX
/// applies no timezone semantics (spec §1). The wrapper exists so a `time` instant
/// cannot be swapped for an arbitrary integer at a call site (parse-don't-validate).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Timestamp(OffsetDateTime);

impl Timestamp {
    /// Builds a timestamp from a raw tick count in the given [`TimeUnit`].
    ///
    /// The tick count is the parquet/arrow physical value (e.g. microseconds since
    /// the Unix epoch for `timestamp[us]`); it is widened to nanoseconds and read as
    /// a UTC instant.
    ///
    /// # Errors
    ///
    /// | Condition | Error |
    /// |---|---|
    /// | the resulting instant is outside the representable [`OffsetDateTime`] range | [`CoreError::ParquetRead`] (the value cannot be a valid timestamp; `artifact` echoed, `detail` from the conversion) |
    fn from_ticks(artifact: &str, ticks: i64, unit: TimeUnit) -> Result<Self, CoreError> {
        let nanos: i128 = match unit {
            TimeUnit::Second => (ticks as i128) * 1_000_000_000,
            TimeUnit::Millisecond => (ticks as i128) * 1_000_000,
            TimeUnit::Microsecond => (ticks as i128) * 1_000,
            TimeUnit::Nanosecond => ticks as i128,
        };
        OffsetDateTime::from_unix_timestamp_nanos(nanos)
            .map(Timestamp)
            .map_err(|e| CoreError::ParquetRead {
                artifact: artifact.to_string(),
                detail: format!("time value {ticks} ({unit:?}) is not a valid timestamp: {e}"),
            })
    }

    /// Returns the timestamp as a UTC [`OffsetDateTime`].
    pub fn as_offset_date_time(&self) -> OffsetDateTime {
        self.0
    }
}

/// Which path produced a [`TimeExtent`] (spec §8).
///
/// An enum, never a `bool`, so the provenance is self-documenting at every call site
/// (architecture §3.3). Recorded so downstream consumers can report which tier
/// produced each extent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeExtentSource {
    /// The extent came from the `time` column's **row-group min/max statistics** —
    /// a pure footer/metadata read, no data decoded (spec §8, the primary path).
    Statistics,
    /// The extent came from the **bounded 1-D `time`-column scan** fallback: the
    /// `time` column carried no row-group statistics, so the reader projected only
    /// that one column by name and read it. This is an architecture-§1-compliant
    /// metadata/index-tier read (a 1-D coordinate read), **not** a gridded-chunk
    /// value decode.
    BoundedColumnScan,
}

/// A per-basin time extent: `[start, end]` plus the [`TimeExtentSource`] (spec §6.1).
///
/// Records the basin's ragged time span as a *fact*; HDX enforces nothing about it
/// (basins may differ in period of record — spec §6.1). It is **inert/agnostic**
/// (spec §1): two timestamps and their provenance, nothing derived.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeExtent {
    start: Timestamp,
    end: Timestamp,
    source: TimeExtentSource,
}

impl TimeExtent {
    /// Returns the earliest timestamp on the basin's `time` axis.
    pub fn start(&self) -> Timestamp {
        self.start
    }

    /// Returns the latest timestamp on the basin's `time` axis.
    pub fn end(&self) -> Timestamp {
        self.end
    }

    /// Returns which path produced this extent (statistics vs the bounded fallback).
    pub fn source(&self) -> TimeExtentSource {
        self.source
    }
}

/// The `time` column descriptor of a `scalar_dynamic.parquet` (spec §6).
///
/// Records the four facts T1 will later check: the column [`name`](Self::name), its
/// logical [`Dtype`] (a full timestamp, spec §6.3), its [`nullability`](Self::is_nullable),
/// and whether it is [`sorted ascending`](Self::is_sorted_ascending). This descriptor
/// **records** these facts; it enforces none of them (spec §1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimeColumn {
    name: String,
    dtype: Dtype,
    nullable: bool,
    sorted_ascending: bool,
}

impl TimeColumn {
    /// Borrows the column name as read from the arrow schema (expected `time`).
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the logical dtype of the column (expected [`Dtype::Timestamp`]).
    pub fn dtype(&self) -> Dtype {
        self.dtype
    }

    /// Returns `true` iff the arrow schema marks the column nullable (T1 expects
    /// non-nullable — spec §6.3).
    pub fn is_nullable(&self) -> bool {
        self.nullable
    }

    /// Returns `true` iff the `time` axis is sorted strictly non-decreasing, as
    /// observed from row-group statistics or the bounded `time`-only scan (spec §6.3).
    pub fn is_sorted_ascending(&self) -> bool {
        self.sorted_ascending
    }

    /// Test-only: builds a [`TimeColumn`] from its four recorded facts.
    ///
    /// The production path constructs a [`TimeColumn`] only from a parsed arrow schema
    /// (see [`read_scalar_dynamic`]); this constructor exists so the in-memory T1
    /// negative tests can hand-build a non-conformant descriptor (a nullable /
    /// unsorted / mis-named / mis-typed `time`) without differently-shaped on-disk
    /// bytes. It is `#[cfg(test)]`-gated, so it adds no production surface.
    #[cfg(test)]
    pub(crate) fn new_for_test(
        name: impl Into<String>,
        dtype: Dtype,
        nullable: bool,
        sorted_ascending: bool,
    ) -> Self {
        Self {
            name: name.into(),
            dtype,
            nullable,
            sorted_ascending,
        }
    }
}

/// The discovered facts of the dataset-level `scalar_static.parquet` rollup
/// (spec §4: one row per basin; cols = `basin_id` + static scalar fields).
///
/// Holds the [`Quadrant::ScalarStatic`] field catalog and whether the `basin_id`
/// column is present (spec §3/§14 I1). It records facts; it enforces nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScalarStaticTable {
    fields: Vec<Field>,
    has_basin_id: bool,
}

impl ScalarStaticTable {
    /// Borrows the static-scalar field catalog (the ordinary data columns).
    pub fn fields(&self) -> &[Field] {
        &self.fields
    }

    /// Returns `true` iff the `basin_id` column is present in the rollup (I1).
    pub fn has_basin_id(&self) -> bool {
        self.has_basin_id
    }
}

/// The discovered facts of one per-basin `scalar_dynamic.parquet`
/// (spec §4: rows = `time`; cols = `basin_id` + dynamic scalar fields).
///
/// Holds the [`Quadrant::ScalarDynamic`] field catalog, the `basin_id` column
/// presence + its distinct in-file value(s), and the [`TimeColumn`] descriptor.
/// It records facts (I1/I2/I3/T1 inputs); it enforces nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScalarDynamicTable {
    fields: Vec<Field>,
    has_basin_id: bool,
    basin_id_values: Vec<BasinId>,
    time: TimeColumn,
}

impl ScalarDynamicTable {
    /// Borrows the dynamic-scalar field catalog (the ordinary data columns).
    pub fn fields(&self) -> &[Field] {
        &self.fields
    }

    /// Returns `true` iff the `basin_id` column is present (spec §3/§14 I1).
    pub fn has_basin_id(&self) -> bool {
        self.has_basin_id
    }

    /// Borrows the distinct in-file `basin_id` value(s) read from the column.
    ///
    /// This is the **authoritative** id (spec §3); the I2 cross-check pairs it with
    /// the `basin=<id>` folder id. A conformant per-basin table holds exactly one
    /// distinct value; this reader records whatever it finds and decides nothing.
    pub fn basin_id_values(&self) -> &[BasinId] {
        &self.basin_id_values
    }

    /// Borrows the `time` column descriptor (spec §6/§14 T1).
    pub fn time(&self) -> &TimeColumn {
        &self.time
    }
}

/// Maps an arrow [`DataType`] to the canonical dtype string [`parse_dtype`] accepts.
///
/// This is the single documented bridge from the physical arrow type the parquet
/// schema surfaces to HDX's closed [`Dtype`] set. The mapping is intentionally
/// narrow — HDX recognizes exactly the encodings the spec admits and rejects
/// everything else, so the dtype set stays closed and semantics-opaque (spec §2).
///
/// | Arrow `DataType` | canonical string |
/// |---|---|
/// | `Float32` | `f32` |
/// | `Float64` | `f64` |
/// | `Int32` | `i32` |
/// | `Int64` | `i64` |
/// | `Boolean` | `bool` |
/// | `Timestamp(_, _)` (any unit / zone) | `timestamp` |
///
/// Returns `None` for any other arrow type, so the caller surfaces a typed
/// [`CoreError::UnknownDtype`] rather than inventing a mapping.
fn arrow_dtype_str(data_type: &DataType) -> Option<&'static str> {
    match data_type {
        DataType::Float32 => Some("f32"),
        DataType::Float64 => Some("f64"),
        DataType::Int32 => Some("i32"),
        DataType::Int64 => Some("i64"),
        DataType::Boolean => Some("bool"),
        DataType::Timestamp(_, _) => Some("timestamp"),
        _ => None,
    }
}

/// Parses an arrow [`DataType`] into a closed [`Dtype`] at the boundary.
///
/// # Errors
///
/// | Condition | Error |
/// |---|---|
/// | the arrow type does not map to a supported encoding (see [`arrow_dtype_str`]) | [`CoreError::UnknownDtype`] (with `found` echoing the arrow type's debug string) |
fn arrow_dtype(data_type: &DataType) -> Result<Dtype, CoreError> {
    match arrow_dtype_str(data_type) {
        Some(s) => parse_dtype(s),
        None => Err(CoreError::UnknownDtype {
            found: format!("{data_type:?}"),
        }),
    }
}

/// Maps the arrow schema's data columns to an ordinary [`Field`] catalog.
///
/// Every column except `basin_id` and `time` becomes one scalar [`Field`] with the
/// supplied `quadrant`, its [`Dtype`] from [`arrow_dtype`], [`Units::none`] (parquet
/// column metadata carries no units in the fixture — recorded as absent, never
/// invented), and `grid_label: None` (scalar). Field names are taken **verbatim**
/// from the schema — no name-pattern special-casing (spec §2).
///
/// # Errors
///
/// | Condition | Error |
/// |---|---|
/// | a data column's arrow type does not map to a supported [`Dtype`] | [`CoreError::UnknownDtype`] |
/// | the resulting scalar field somehow fails its construction invariant | [`CoreError::MismatchedGridLabel`] (cannot occur for a scalar field with no label, but propagated rather than panicked) |
fn catalog_fields(schema: &Schema, quadrant: Quadrant) -> Result<Vec<Field>, CoreError> {
    schema
        .fields()
        .iter()
        .filter(|f| f.name() != BASIN_ID_COLUMN && f.name() != TIME_COLUMN)
        .map(|f| {
            let dtype = arrow_dtype(f.data_type())?;
            // Scalar field: no grid label. `Field::new` enforces the invariant.
            Field::new(
                FieldName::new(f.name()),
                quadrant,
                dtype,
                Units::none(),
                None,
            )
        })
        .collect()
}

/// Reads `bytes` of a parquet artifact into a `parquet` reader builder.
///
/// Centralizes the open + typed-error mapping so both the schema reads and the
/// bounded fallback share one entry point. Decodes the footer/metadata only.
///
/// # Errors
///
/// | Condition | Error |
/// |---|---|
/// | `bytes` is not a valid parquet file, or its footer/metadata fails to decode | [`CoreError::ParquetRead`] |
fn open_builder(
    artifact: &str,
    bytes: Bytes,
) -> Result<ParquetRecordBatchReaderBuilder<Bytes>, CoreError> {
    ParquetRecordBatchReaderBuilder::try_new(bytes).map_err(|e| CoreError::ParquetRead {
        artifact: artifact.to_string(),
        detail: e.to_string(),
    })
}

/// Reads a file at `path` into bytes, mapping IO failure to a typed error.
///
/// # Errors
///
/// | Condition | Error |
/// |---|---|
/// | the file cannot be read (missing / not readable) | [`CoreError::ParquetRead`] |
fn read_file_bytes(path: &Path) -> Result<Bytes, CoreError> {
    fs::read(path)
        .map(Bytes::from)
        .map_err(|e| CoreError::ParquetRead {
            artifact: path.display().to_string(),
            detail: e.to_string(),
        })
}

/// Returns `true` iff the schema carries a column named `basin_id` (spec §3/§14 I1).
fn schema_has_basin_id(schema: &Schema) -> bool {
    schema.fields().iter().any(|f| f.name() == BASIN_ID_COLUMN)
}

/// Reads `scalar_static.parquet` metadata into a [`ScalarStaticTable`] (spec §4).
///
/// Opens the parquet footer (no data decoded), maps every non-`basin_id` column to
/// a [`Quadrant::ScalarStatic`] [`Field`], and records `basin_id` presence (I1).
///
/// # Errors
///
/// | Condition | Error |
/// |---|---|
/// | the file cannot be read, or its parquet metadata fails to decode | [`CoreError::ParquetRead`] |
/// | a data column's arrow type does not map to a supported [`Dtype`] | [`CoreError::UnknownDtype`] |
#[instrument(fields(path = %path.as_ref().display()))]
pub fn read_scalar_static(path: impl AsRef<Path>) -> Result<ScalarStaticTable, CoreError> {
    let path = path.as_ref();
    let artifact = path.display().to_string();
    let bytes = read_file_bytes(path)?;
    // Metadata only (architecture §1): the schema, no data decoded.
    let meta = read_parquet_meta(&artifact, &bytes)?;
    let schema = meta.schema();

    let fields = catalog_fields(schema, Quadrant::ScalarStatic)?;
    let has_basin_id = schema_has_basin_id(schema);

    info!(
        fields = fields.len(),
        has_basin_id, "read scalar_static metadata"
    );
    Ok(ScalarStaticTable {
        fields,
        has_basin_id,
    })
}

/// Reads a `scalar_dynamic.parquet` metadata into a [`ScalarDynamicTable`] (spec §4).
///
/// Opens the parquet footer, maps every non-`basin_id`/non-`time` column to a
/// [`Quadrant::ScalarDynamic`] [`Field`], records `basin_id` presence + its distinct
/// in-file value(s) (a 1-D key-column read, architecture §1), and builds the
/// [`TimeColumn`] descriptor (name / dtype / nullability / sort).
///
/// The `time` column is **structurally required** here (spec §6): a dynamic table
/// without it is reported via [`CoreError::MissingScalarColumn`], not panicked over.
///
/// # Errors
///
/// | Condition | Error |
/// |---|---|
/// | the file cannot be read, or its parquet metadata fails to decode | [`CoreError::ParquetRead`] |
/// | the `time` column is absent from the schema | [`CoreError::MissingScalarColumn`] |
/// | a data column's (or the `time` column's) arrow type does not map to a supported [`Dtype`] | [`CoreError::UnknownDtype`] |
#[instrument(fields(path = %path.as_ref().display()))]
pub fn read_scalar_dynamic(path: impl AsRef<Path>) -> Result<ScalarDynamicTable, CoreError> {
    let path = path.as_ref();
    let artifact = path.display().to_string();
    let bytes = read_file_bytes(path)?;
    // Metadata only first (architecture §1): schema + footer for the `time` sort and
    // descriptor. The `basin_id` value(s) need the bounded key-column read below.
    let meta = read_parquet_meta(&artifact, &bytes)?;
    let schema = meta.schema().clone();
    let metadata = meta.file_metadata();

    let fields = catalog_fields(&schema, Quadrant::ScalarDynamic)?;
    let has_basin_id = schema_has_basin_id(&schema);

    // The `time` descriptor: name + nullability from the arrow schema, dtype via the
    // arrow-type mapping, sort from row-group statistics (or the bounded scan fallback).
    let time_field = schema
        .fields()
        .iter()
        .find(|f| f.name() == TIME_COLUMN)
        .ok_or_else(|| CoreError::MissingScalarColumn {
            artifact: artifact.clone(),
            column: TIME_COLUMN.to_string(),
        })?;
    let time_dtype = arrow_dtype(time_field.data_type())?;
    let time_nullable = time_field.is_nullable();
    let sorted_ascending = time_sorted_ascending(&artifact, metadata)?;

    let time = TimeColumn {
        name: time_field.name().to_string(),
        dtype: time_dtype,
        nullable: time_nullable,
        sorted_ascending,
    };

    // The distinct in-file `basin_id` value(s): a bounded 1-D key-column read.
    let basin_id_values = if has_basin_id {
        read_basin_id_values_from_bytes(&artifact, bytes, metadata)?
    } else {
        Vec::new()
    };

    info!(
        fields = fields.len(),
        has_basin_id,
        basin_ids = basin_id_values.len(),
        time_sorted = sorted_ascending,
        "read scalar_dynamic metadata"
    );
    Ok(ScalarDynamicTable {
        fields,
        has_basin_id,
        basin_id_values,
        time,
    })
}

/// Locates a leaf column index in the parquet schema by name, or `None`.
///
/// Used to project a single column by name for the bounded reads (the `time`-only
/// fallback and the `basin_id` key-column read), so no other column is touched.
fn leaf_column_index(metadata: &ParquetMetaData, name: &str) -> Option<usize> {
    metadata
        .file_metadata()
        .schema_descr()
        .columns()
        .iter()
        .position(|c| c.name() == name)
}

/// Returns the `time` row-group min/max statistics across all row groups, if every
/// row group exposes usable statistics for the `time` column (spec §8).
///
/// Returns `None` if any row group lacks `time` statistics, signaling the caller to
/// use the bounded fallback. The `time` column physical type is `Int64` (a parquet
/// `Timestamp` is encoded as `INT64`), so the statistics arrive as
/// [`Statistics::Int64`]; the tick count is interpreted via the schema's [`TimeUnit`].
fn time_stats_extent(
    artifact: &str,
    metadata: &ParquetMetaData,
    unit: TimeUnit,
) -> Result<Option<(Timestamp, Timestamp)>, CoreError> {
    let Some(col_idx) = leaf_column_index(metadata, TIME_COLUMN) else {
        return Ok(None);
    };
    if metadata.num_row_groups() == 0 {
        return Ok(None);
    }

    let mut overall_min: Option<i64> = None;
    let mut overall_max: Option<i64> = None;

    for rg in metadata.row_groups() {
        let column = rg.column(col_idx);
        let Some(stats) = column.statistics() else {
            return Ok(None);
        };
        let Statistics::Int64(value_stats) = stats else {
            // A non-Int64 time statistic is unexpected for a timestamp column; treat
            // it as "no usable statistics" and let the bounded fallback recover it.
            return Ok(None);
        };
        let (Some(&min), Some(&max)) = (value_stats.min_opt(), value_stats.max_opt()) else {
            return Ok(None);
        };
        overall_min = Some(overall_min.map_or(min, |m: i64| m.min(min)));
        overall_max = Some(overall_max.map_or(max, |m: i64| m.max(max)));
    }

    match (overall_min, overall_max) {
        (Some(min), Some(max)) => {
            let start = Timestamp::from_ticks(artifact, min, unit)?;
            let end = Timestamp::from_ticks(artifact, max, unit)?;
            Ok(Some((start, end)))
        }
        _ => Ok(None),
    }
}

/// Reads the `time` column (and only that column) into its UTC timestamps, bounded.
///
/// This is the **bounded fallback** read: it projects **exactly the `time` column by
/// name** ([`ProjectionMask::leaves`] over a single leaf index) and reads it. No data
/// column is ever projected, and no gridded chunk is touched — this is a 1-D
/// coordinate read (architecture §1). Returns the timestamps in file order so the
/// caller can both bound them and check ascending order.
///
/// # Errors
///
/// | Condition | Error |
/// |---|---|
/// | the `time` column is absent from the schema | [`CoreError::MissingScalarColumn`] |
/// | the parquet reader fails, or the `time` column is not a supported timestamp array | [`CoreError::ParquetRead`] |
fn read_time_column(
    artifact: &str,
    bytes: Bytes,
    metadata: &ParquetMetaData,
    unit: TimeUnit,
) -> Result<Vec<Timestamp>, CoreError> {
    let col_idx =
        leaf_column_index(metadata, TIME_COLUMN).ok_or_else(|| CoreError::MissingScalarColumn {
            artifact: artifact.to_string(),
            column: TIME_COLUMN.to_string(),
        })?;

    let builder = open_builder(artifact, bytes)?;
    let mask = ProjectionMask::leaves(builder.parquet_schema(), [col_idx]);
    let reader = builder
        .with_projection(mask)
        .build()
        .map_err(|e| CoreError::ParquetRead {
            artifact: artifact.to_string(),
            detail: e.to_string(),
        })?;

    debug!(
        column = TIME_COLUMN,
        leaf = col_idx,
        "bounded 1-D time-only column read (statistics-absent fallback)"
    );

    let mut timestamps: Vec<Timestamp> = Vec::new();
    for batch in reader {
        let batch = batch.map_err(|e| CoreError::ParquetRead {
            artifact: artifact.to_string(),
            detail: e.to_string(),
        })?;
        // The projected batch has exactly the one `time` column at index 0.
        let array = batch.column(0);
        for tick in timestamp_ticks(artifact, array.as_ref(), unit)? {
            timestamps.push(Timestamp::from_ticks(artifact, tick, unit)?);
        }
    }
    Ok(timestamps)
}

/// Extracts the raw i64 tick counts from a timestamp arrow array of any unit.
///
/// # Errors
///
/// | Condition | Error |
/// |---|---|
/// | the array is not a timestamp array of the schema's unit, or a value is null | [`CoreError::ParquetRead`] (a non-timestamp `time` column / null is a structural surprise the bounded read cannot interpret) |
fn timestamp_ticks(
    artifact: &str,
    array: &dyn Array,
    unit: TimeUnit,
) -> Result<Vec<i64>, CoreError> {
    fn collect<A: Array + 'static>(
        artifact: &str,
        array: &dyn Array,
        get: impl Fn(&A, usize) -> i64,
    ) -> Result<Vec<i64>, CoreError> {
        let typed = array
            .as_any()
            .downcast_ref::<A>()
            .ok_or_else(|| CoreError::ParquetRead {
                artifact: artifact.to_string(),
                detail: "time column is not the expected timestamp array type".to_string(),
            })?;
        let mut out = Vec::with_capacity(typed.len());
        for i in 0..typed.len() {
            if typed.is_null(i) {
                return Err(CoreError::ParquetRead {
                    artifact: artifact.to_string(),
                    detail: "time column carries a null value".to_string(),
                });
            }
            out.push(get(typed, i));
        }
        Ok(out)
    }

    match unit {
        TimeUnit::Second => collect::<TimestampSecondArray>(artifact, array, |a, i| a.value(i)),
        TimeUnit::Millisecond => {
            collect::<TimestampMillisecondArray>(artifact, array, |a, i| a.value(i))
        }
        TimeUnit::Microsecond => {
            collect::<TimestampMicrosecondArray>(artifact, array, |a, i| a.value(i))
        }
        TimeUnit::Nanosecond => {
            collect::<TimestampNanosecondArray>(artifact, array, |a, i| a.value(i))
        }
    }
}

/// Reads the distinct in-file `basin_id` value(s) via a bounded `basin_id`-only read.
///
/// Like the `time` fallback, this projects **only the `basin_id` column by name**
/// (architecture §1: a 1-D key-column read, never a data column). Values are
/// returned distinct, in first-seen order. The `basin_id` column is a parquet
/// `BYTE_ARRAY` / arrow `Utf8` (or `LargeUtf8`); each row is wrapped verbatim into a
/// [`BasinId`] (spec §3 — HDX parses none of its contents). It reads the value, not
/// the schema, so the caller supplies the file `bytes` it already holds.
///
/// # Errors
///
/// | Condition | Error |
/// |---|---|
/// | the `basin_id` column is absent from the schema | [`CoreError::MissingScalarColumn`] |
/// | the parquet reader fails, or `basin_id` is not a string array | [`CoreError::ParquetRead`] |
fn read_basin_id_values_from_bytes(
    artifact: &str,
    bytes: Bytes,
    metadata: &ParquetMetaData,
) -> Result<Vec<BasinId>, CoreError> {
    let col_idx = leaf_column_index(metadata, BASIN_ID_COLUMN).ok_or_else(|| {
        CoreError::MissingScalarColumn {
            artifact: artifact.to_string(),
            column: BASIN_ID_COLUMN.to_string(),
        }
    })?;

    let builder = open_builder(artifact, bytes)?;
    let mask = ProjectionMask::leaves(builder.parquet_schema(), [col_idx]);
    let reader = builder
        .with_projection(mask)
        .build()
        .map_err(|e| CoreError::ParquetRead {
            artifact: artifact.to_string(),
            detail: e.to_string(),
        })?;

    debug!(
        column = BASIN_ID_COLUMN,
        leaf = col_idx,
        "bounded 1-D basin_id-only key-column read"
    );

    let mut seen: Vec<BasinId> = Vec::new();
    for batch in reader {
        let batch = batch.map_err(|e| CoreError::ParquetRead {
            artifact: artifact.to_string(),
            detail: e.to_string(),
        })?;
        let array = batch.column(0);
        for value in string_values(artifact, array.as_ref())? {
            let id = BasinId::new(value);
            if !seen.contains(&id) {
                seen.push(id);
            }
        }
    }
    Ok(seen)
}

/// Extracts the string values of a `Utf8` / `LargeUtf8` arrow array (the `basin_id`
/// column), skipping nulls (a null id is recorded as absent, never invented).
///
/// # Errors
///
/// | Condition | Error |
/// |---|---|
/// | the array is neither `Utf8` nor `LargeUtf8` | [`CoreError::ParquetRead`] (an unexpected `basin_id` encoding) |
fn string_values(artifact: &str, array: &dyn Array) -> Result<Vec<String>, CoreError> {
    use arrow::array::{LargeStringArray, StringArray};

    if let Some(a) = array.as_any().downcast_ref::<StringArray>() {
        Ok((0..a.len())
            .filter(|&i| !a.is_null(i))
            .map(|i| a.value(i).to_string())
            .collect())
    } else if let Some(a) = array.as_any().downcast_ref::<LargeStringArray>() {
        Ok((0..a.len())
            .filter(|&i| !a.is_null(i))
            .map(|i| a.value(i).to_string())
            .collect())
    } else {
        Err(CoreError::ParquetRead {
            artifact: artifact.to_string(),
            detail: "basin_id column is not a string array".to_string(),
        })
    }
}

/// Resolves the schema's `time` [`TimeUnit`], defaulting to microseconds (spec §6.3).
///
/// The fixture uses `timestamp[us]`; if a producer wrote a different unit the reader
/// honors it. A non-timestamp `time` column has no unit and yields `None`.
fn time_unit(schema: &Schema) -> Option<TimeUnit> {
    schema
        .fields()
        .iter()
        .find(|f| f.name() == TIME_COLUMN)
        .and_then(|f| match f.data_type() {
            DataType::Timestamp(unit, _) => Some(*unit),
            _ => None,
        })
}

/// Determines whether the `time` axis is sorted ascending, statistics-first.
///
/// Uses per-row-group min/max statistics where present (each row group's min ≥ the
/// previous row group's max, and min ≤ max within a group — for a single row group
/// this is trivially satisfied by usable statistics). When statistics are absent it
/// is *not* decided here (the bounded scan in [`time_extent`] establishes ordering);
/// in that case this conservatively returns `false` so the descriptor records the
/// fact that sort could not be confirmed from metadata. T1 is enforced elsewhere.
fn time_sorted_ascending(_artifact: &str, metadata: &ParquetMetaData) -> Result<bool, CoreError> {
    let Some(col_idx) = leaf_column_index(metadata, TIME_COLUMN) else {
        return Ok(false);
    };
    if metadata.num_row_groups() == 0 {
        return Ok(false);
    }

    let mut prev_max: Option<i64> = None;
    for rg in metadata.row_groups() {
        let column = rg.column(col_idx);
        let Some(Statistics::Int64(stats)) = column.statistics() else {
            return Ok(false);
        };
        let (Some(&min), Some(&max)) = (stats.min_opt(), stats.max_opt()) else {
            return Ok(false);
        };
        if min > max {
            return Ok(false);
        }
        if let Some(prev) = prev_max
            && min < prev
        {
            return Ok(false);
        }
        prev_max = Some(max);
    }
    Ok(true)
}

/// Computes a basin's `[start, end]` time extent, statistics-first (spec §6.1/§8).
///
/// **Statistics path (primary, spec §8).** Reads the `time` column's row-group
/// min/max statistics and returns `[min, max]` with [`TimeExtentSource::Statistics`]
/// — a pure footer read, no data decoded.
///
/// **Bounded fallback.** When the `time` column carries no usable row-group
/// statistics, projects **only the `time` column by name** and reads it to recover
/// `[min, max]`, returning [`TimeExtentSource::BoundedColumnScan`]. This is an
/// architecture-§1-compliant 1-D coordinate/key-column read, **not** a gridded-chunk
/// value decode. The bound is hard: exactly one column.
///
/// # Errors
///
/// | Condition | Error |
/// |---|---|
/// | the file cannot be read, or its parquet metadata fails to decode | [`CoreError::ParquetRead`] |
/// | the `time` column is absent from the schema | [`CoreError::MissingScalarColumn`] |
/// | a `time` value is outside the representable timestamp range | [`CoreError::ParquetRead`] |
/// | the table has no rows on the fallback path (no extent exists) | [`CoreError::ParquetRead`] (reports the empty `time` axis) |
#[instrument(fields(path = %path.as_ref().display()))]
pub fn time_extent(path: impl AsRef<Path>) -> Result<TimeExtent, CoreError> {
    let path = path.as_ref();
    let artifact = path.display().to_string();
    let bytes = read_file_bytes(path)?;
    // Metadata only first (architecture §1): the schema + footer statistics.
    let meta = read_parquet_meta(&artifact, &bytes)?;
    let schema = meta.schema();
    let metadata = meta.file_metadata();

    // The `time` column must exist structurally for an extent to be meaningful.
    let unit = time_unit(schema).ok_or_else(|| CoreError::MissingScalarColumn {
        artifact: artifact.clone(),
        column: TIME_COLUMN.to_string(),
    })?;

    // Primary: row-group statistics (spec §8).
    if let Some((start, end)) = time_stats_extent(&artifact, metadata, unit)? {
        debug!("time extent from row-group statistics");
        return Ok(TimeExtent {
            start,
            end,
            source: TimeExtentSource::Statistics,
        });
    }

    // Bounded fallback: read ONLY the `time` column.
    warn!(
        column = TIME_COLUMN,
        "time row-group statistics absent; using bounded 1-D time-only fallback"
    );
    let timestamps = read_time_column(&artifact, bytes, metadata, unit)?;
    let start = *timestamps
        .iter()
        .min()
        .ok_or_else(|| CoreError::ParquetRead {
            artifact: artifact.clone(),
            detail: "time column is empty; no extent exists".to_string(),
        })?;
    let end = *timestamps
        .iter()
        .max()
        .ok_or_else(|| CoreError::ParquetRead {
            artifact: artifact.clone(),
            detail: "time column is empty; no extent exists".to_string(),
        })?;
    Ok(TimeExtent {
        start,
        end,
        source: TimeExtentSource::BoundedColumnScan,
    })
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use arrow::array::{Float64Array, StringArray, TimestampMicrosecondArray};
    use arrow::datatypes::{DataType, Field as ArrowField, Schema, TimeUnit};
    use arrow::record_batch::RecordBatch;
    use parquet::arrow::ArrowWriter;
    use parquet::file::properties::{EnabledStatistics, WriterProperties};
    use time::OffsetDateTime;
    use time::format_description::well_known::Rfc3339;

    use crate::error::CoreError;
    use crate::field::{Dtype, Quadrant};
    use crate::newtypes::BasinId;
    use crate::scalar_reader::{
        TimeExtentSource, read_scalar_dynamic, read_scalar_static, time_extent,
    };

    /// Resolves a path under the committed `conformance/` fixture tree.
    fn conformance(rel: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../conformance")
            .join(rel)
    }

    /// Parses an RFC 3339 datetime for asserting exact time-extent boundaries.
    fn rfc3339(s: &str) -> OffsetDateTime {
        OffsetDateTime::parse(s, &Rfc3339).expect("test datetime must parse")
    }

    /// One microsecond-since-epoch tick for a calendar instant, for in-test parquet.
    fn micros(s: &str) -> i64 {
        let dt = rfc3339(s);
        (dt.unix_timestamp_nanos() / 1_000) as i64
    }

    /// Writes a per-basin-style `scalar_dynamic` parquet to memory, with control over
    /// row-group statistics and the data-column name (for the discipline tests).
    fn write_dynamic_parquet(
        basin_id: &str,
        times_micros: &[i64],
        data_col: &str,
        stats: EnabledStatistics,
    ) -> Vec<u8> {
        let schema = Arc::new(Schema::new(vec![
            ArrowField::new("basin_id", DataType::Utf8, false),
            ArrowField::new(
                "time",
                DataType::Timestamp(TimeUnit::Microsecond, None),
                false,
            ),
            ArrowField::new(data_col, DataType::Float64, true),
        ]));
        let n = times_micros.len();
        let ids = StringArray::from(vec![basin_id; n]);
        let times = TimestampMicrosecondArray::from(times_micros.to_vec());
        let data = Float64Array::from((0..n).map(|i| i as f64).collect::<Vec<_>>());
        let batch = RecordBatch::try_new(
            Arc::clone(&schema),
            vec![Arc::new(ids), Arc::new(times), Arc::new(data)],
        )
        .expect("batch must build");

        let props = WriterProperties::builder()
            .set_statistics_enabled(stats)
            .build();
        let mut buffer: Vec<u8> = Vec::new();
        {
            let mut writer = ArrowWriter::try_new(&mut buffer, schema, Some(props))
                .expect("writer must construct");
            writer.write(&batch).expect("write must succeed");
            writer.close().expect("close must succeed");
        }
        buffer
    }

    /// Writes a parquet with no `time` column (for the negative test).
    fn write_timeless_parquet() -> Vec<u8> {
        let schema = Arc::new(Schema::new(vec![
            ArrowField::new("basin_id", DataType::Utf8, false),
            ArrowField::new("streamflow", DataType::Float64, true),
        ]));
        let ids = StringArray::from(vec!["0001", "0001"]);
        let data = Float64Array::from(vec![1.0, 2.0]);
        let batch = RecordBatch::try_new(Arc::clone(&schema), vec![Arc::new(ids), Arc::new(data)])
            .expect("batch must build");
        let mut buffer: Vec<u8> = Vec::new();
        {
            let mut writer =
                ArrowWriter::try_new(&mut buffer, schema, None).expect("writer must construct");
            writer.write(&batch).expect("write must succeed");
            writer.close().expect("close must succeed");
        }
        buffer
    }

    /// Writes a parquet with a `string`-typed data column (an unmapped dtype).
    fn write_unmapped_dtype_parquet() -> Vec<u8> {
        let schema = Arc::new(Schema::new(vec![
            ArrowField::new("basin_id", DataType::Utf8, false),
            ArrowField::new(
                "time",
                DataType::Timestamp(TimeUnit::Microsecond, None),
                false,
            ),
            // A `Utf8` data column does not map to any HDX dtype.
            ArrowField::new("label", DataType::Utf8, true),
        ]));
        let ids = StringArray::from(vec!["0001"]);
        let times = TimestampMicrosecondArray::from(vec![micros("2000-01-01T00:00:00Z")]);
        let labels = StringArray::from(vec!["x"]);
        let batch = RecordBatch::try_new(
            Arc::clone(&schema),
            vec![Arc::new(ids), Arc::new(times), Arc::new(labels)],
        )
        .expect("batch must build");
        let mut buffer: Vec<u8> = Vec::new();
        {
            let mut writer =
                ArrowWriter::try_new(&mut buffer, schema, None).expect("writer must construct");
            writer.write(&batch).expect("write must succeed");
            writer.close().expect("close must succeed");
        }
        buffer
    }

    /// Writes `bytes` to a uniquely-named temp file and returns its path.
    fn temp_parquet(tag: &str, bytes: &[u8]) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "hdx-scalar-{tag}-{}-{}.parquet",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::write(&path, bytes).expect("temp parquet write must succeed");
        path
    }

    // --- Field catalog over the real fixture ------------------------------------

    #[test]
    fn scalar_static_catalogs_drainage_area_and_recognizes_basin_id() {
        let table = read_scalar_static(conformance("valid/minimal/scalar_static.parquet"))
            .expect("scalar_static must read");

        // basin_id is recognized as the id column, not catalogued as a data field.
        assert!(table.has_basin_id());
        assert_eq!(table.fields().len(), 1, "exactly one data field");
        let field = &table.fields()[0];
        assert_eq!(field.name().as_str(), "drainage_area");
        assert_eq!(field.quadrant(), Quadrant::ScalarStatic);
        assert_eq!(field.dtype(), Dtype::F64);
        assert_eq!(
            field.grid_label(),
            None,
            "a scalar field carries no grid label"
        );
    }

    #[test]
    fn scalar_dynamic_catalogs_streamflow_recognizes_basin_id_and_time() {
        let table = read_scalar_dynamic(conformance(
            "valid/minimal/basin=0001/scalar_dynamic.parquet",
        ))
        .expect("scalar_dynamic must read");

        assert!(table.has_basin_id());
        assert_eq!(table.fields().len(), 1, "exactly one data field");
        let field = &table.fields()[0];
        assert_eq!(field.name().as_str(), "streamflow");
        assert_eq!(field.quadrant(), Quadrant::ScalarDynamic);
        assert_eq!(field.dtype(), Dtype::F64);

        // `time` is recognized as the time column (descriptor present, not a field).
        assert_eq!(table.time().name(), "time");
    }

    #[test]
    fn basin_id_in_file_value_for_basin_0001_is_recorded() {
        // I2 input (recorded, NOT compared to the folder here).
        let table = read_scalar_dynamic(conformance(
            "valid/minimal/basin=0001/scalar_dynamic.parquet",
        ))
        .expect("scalar_dynamic must read");
        assert_eq!(
            table.basin_id_values(),
            &[BasinId::new("0001")],
            "exactly one distinct in-file basin_id, recorded for the I2 cross-check"
        );
    }

    #[test]
    fn time_descriptor_is_timestamp_non_nullable_sorted_ascending() {
        // T1 input (recorded, NOT enforced here).
        let table = read_scalar_dynamic(conformance(
            "valid/minimal/basin=0001/scalar_dynamic.parquet",
        ))
        .expect("scalar_dynamic must read");
        let time = table.time();
        assert_eq!(time.name(), "time");
        assert_eq!(time.dtype(), Dtype::Timestamp);
        assert!(
            !time.is_nullable(),
            "fixture time is `timestamp[us] not null`"
        );
        assert!(
            time.is_sorted_ascending(),
            "fixture time is sorted ascending"
        );
    }

    // --- Rust-side statistics confirmation --------------------------------------

    #[test]
    fn med5_time_statistics_are_rust_readable_and_extent_is_from_statistics() {
        use bytes::Bytes;
        use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
        use parquet::file::statistics::Statistics;

        let path = conformance("valid/minimal/basin=0001/scalar_dynamic.parquet");

        // 1) Rust-side confirmation: the `time` row-group statistics expose usable
        //    min/max. If this ever fails, the fix is to REGENERATE the fixture,
        //    NEVER a reader workaround — see the module docs.
        let raw = std::fs::read(&path).expect("read fixture bytes");
        let builder =
            ParquetRecordBatchReaderBuilder::try_new(Bytes::from(raw)).expect("fixture must open");
        let metadata = builder.metadata().clone();
        let time_idx = metadata
            .file_metadata()
            .schema_descr()
            .columns()
            .iter()
            .position(|c| c.name() == "time")
            .expect("time column must exist");
        let rg = metadata.row_group(0);
        let stats = rg
            .column(time_idx)
            .statistics()
            .expect("time column must carry Rust-readable row-group statistics");
        match stats {
            Statistics::Int64(s) => {
                assert!(s.min_opt().is_some(), "time min statistic present");
                assert!(s.max_opt().is_some(), "time max statistic present");
            }
            other => panic!("expected Int64 time statistics, got {other:?}"),
        }

        // 2) The extent comes from Statistics (not the fallback), with exact bounds.
        let extent = time_extent(&path).expect("extent must read");
        assert_eq!(
            extent.source(),
            TimeExtentSource::Statistics,
            "the fixture extent MUST come from row-group statistics, not the fallback"
        );
        assert_eq!(
            extent.start().as_offset_date_time(),
            rfc3339("2000-01-01T00:00:00Z")
        );
        assert_eq!(
            extent.end().as_offset_date_time(),
            rfc3339("2000-01-05T00:00:00Z")
        );
    }

    // --- Ragged-across-basins extents (§6.1) ------------------------------------

    #[test]
    fn ragged_extents_differ_across_three_basins() {
        let e1 = time_extent(conformance(
            "valid/minimal/basin=0001/scalar_dynamic.parquet",
        ))
        .expect("0001 extent");
        let e2 = time_extent(conformance(
            "valid/minimal/basin=0002/scalar_dynamic.parquet",
        ))
        .expect("0002 extent");
        let e3 = time_extent(conformance(
            "valid/minimal/basin=0003/scalar_dynamic.parquet",
        ))
        .expect("0003 extent");

        // All three come from statistics on the conformant fixture.
        for e in [&e1, &e2, &e3] {
            assert_eq!(e.source(), TimeExtentSource::Statistics);
        }
        // §6.1: ragged extents surfaced — the three basins (5 / 7 / 4 rows) span
        // entirely different periods of record, so their extents differ.
        assert_eq!(
            e1.start().as_offset_date_time(),
            rfc3339("2000-01-01T00:00:00Z")
        );
        assert_eq!(
            e1.end().as_offset_date_time(),
            rfc3339("2000-01-05T00:00:00Z")
        );
        assert_eq!(
            e2.start().as_offset_date_time(),
            rfc3339("2010-06-15T00:00:00Z")
        );
        assert_eq!(
            e2.end().as_offset_date_time(),
            rfc3339("2010-06-21T00:00:00Z")
        );
        assert_eq!(
            e3.start().as_offset_date_time(),
            rfc3339("2005-03-01T00:00:00Z")
        );
        assert_eq!(
            e3.end().as_offset_date_time(),
            rfc3339("2005-03-04T00:00:00Z")
        );
        assert_ne!(e1.end(), e2.end());
        assert_ne!(e2.end(), e3.end());
        assert_ne!(e1.end(), e3.end());
    }

    // --- Bounded fallback -------------------------------------------------------

    #[test]
    fn low1_fallback_used_when_statistics_disabled_with_correct_bounds() {
        let times = [
            micros("2001-03-01T00:00:00Z"),
            micros("2001-03-02T00:00:00Z"),
            micros("2001-03-03T00:00:00Z"),
        ];
        // Statistics disabled: forces the bounded `time`-only fallback path.
        let bytes = write_dynamic_parquet("0042", &times, "streamflow", EnabledStatistics::None);
        let path = temp_parquet("low1", &bytes);

        let extent = time_extent(&path);
        std::fs::remove_file(&path).ok();
        let extent = extent.expect("fallback extent must read");

        assert_eq!(
            extent.source(),
            TimeExtentSource::BoundedColumnScan,
            "with statistics disabled the extent MUST come from the bounded scan"
        );
        assert_eq!(
            extent.start().as_offset_date_time(),
            rfc3339("2001-03-01T00:00:00Z")
        );
        assert_eq!(
            extent.end().as_offset_date_time(),
            rfc3339("2001-03-03T00:00:00Z")
        );
    }

    #[test]
    fn low1_fallback_extent_is_independent_of_the_data_column() {
        // The fallback projects ONLY `time`. Two assets share identical `time` values
        // but carry different `data` columns; the recovered extent must be identical,
        // proving no data column influences the bounded read.
        let times = [
            micros("2010-06-01T00:00:00Z"),
            micros("2010-06-02T00:00:00Z"),
        ];
        let a = write_dynamic_parquet("0001", &times, "streamflow", EnabledStatistics::None);
        let b = write_dynamic_parquet("0001", &times, "precip", EnabledStatistics::None);
        let pa = temp_parquet("low1a", &a);
        let pb = temp_parquet("low1b", &b);

        let ea = time_extent(&pa);
        let eb = time_extent(&pb);
        std::fs::remove_file(&pa).ok();
        std::fs::remove_file(&pb).ok();
        let ea = ea.expect("a extent");
        let eb = eb.expect("b extent");

        assert_eq!(ea.source(), TimeExtentSource::BoundedColumnScan);
        assert_eq!(eb.source(), TimeExtentSource::BoundedColumnScan);
        assert_eq!(
            ea.start(),
            eb.start(),
            "extent independent of the data column"
        );
        assert_eq!(ea.end(), eb.end(), "extent independent of the data column");
    }

    // --- Ordinary-field discipline (no name magic) ------------------------------

    #[test]
    fn was_filled_named_column_is_an_ordinary_field_with_no_suffix_magic() {
        let times = [micros("2000-01-01T00:00:00Z")];
        // A companion-mask-pattern name must be catalogued as an ordinary field.
        let bytes = write_dynamic_parquet(
            "0001",
            &times,
            "streamflow_was_filled",
            EnabledStatistics::Page,
        );
        let path = temp_parquet("wasfilled", &bytes);

        let table = read_scalar_dynamic(&path);
        std::fs::remove_file(&path).ok();
        let table = table.expect("must read");

        assert_eq!(
            table.fields().len(),
            1,
            "the `_was_filled` column is one ordinary field"
        );
        let field = &table.fields()[0];
        assert_eq!(
            field.name().as_str(),
            "streamflow_was_filled",
            "the name is taken verbatim — no suffix magic, no belongs-to"
        );
        assert_eq!(field.quadrant(), Quadrant::ScalarDynamic);
        assert_eq!(field.dtype(), Dtype::F64);
        assert_eq!(field.grid_label(), None);
    }

    // --- Negative paths ---------------------------------------------------------

    #[test]
    fn dynamic_table_missing_time_returns_missing_scalar_column() {
        let bytes = write_timeless_parquet();
        let path = temp_parquet("notime", &bytes);

        let result = read_scalar_dynamic(&path);
        std::fs::remove_file(&path).ok();

        match result {
            Err(CoreError::MissingScalarColumn { artifact, column }) => {
                assert!(artifact.contains("notime"));
                assert_eq!(column, "time");
            }
            other => panic!("expected MissingScalarColumn, got {other:?}"),
        }
    }

    #[test]
    fn time_extent_on_timeless_table_returns_missing_scalar_column() {
        let bytes = write_timeless_parquet();
        let path = temp_parquet("notime-extent", &bytes);

        let result = time_extent(&path);
        std::fs::remove_file(&path).ok();

        match result {
            Err(CoreError::MissingScalarColumn { column, .. }) => assert_eq!(column, "time"),
            other => panic!("expected MissingScalarColumn, got {other:?}"),
        }
    }

    #[test]
    fn unmapped_arrow_dtype_returns_unknown_dtype() {
        let bytes = write_unmapped_dtype_parquet();
        let path = temp_parquet("unmapped", &bytes);

        let result = read_scalar_dynamic(&path);
        std::fs::remove_file(&path).ok();

        match result {
            Err(CoreError::UnknownDtype { found }) => {
                assert!(
                    found.contains("Utf8"),
                    "the rejected arrow type is echoed: {found}"
                );
            }
            other => panic!("expected UnknownDtype, got {other:?}"),
        }
    }

    #[test]
    fn missing_file_returns_parquet_read() {
        match read_scalar_static("/no/such/scalar_static.parquet") {
            Err(CoreError::ParquetRead { artifact, detail }) => {
                assert!(artifact.contains("scalar_static.parquet"));
                assert!(!detail.is_empty());
            }
            other => panic!("expected ParquetRead, got {other:?}"),
        }
    }
}
