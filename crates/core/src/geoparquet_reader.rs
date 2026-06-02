//! The `outlines.geoparquet` **metadata + 1-D column** reader (MS4-S4; spec §9,
//! architecture §1/§3.5).
//!
//! Reads the plural-outlines artifact into the structural facts the Geo1 / I1 / M5
//! checks (enforcement is MS6) need, **reusing MS3's pure-Rust `parquet`/`arrow`
//! stack** (the R1 decision — no new crate) via the same private `parquet_meta`
//! touchpoint the scalar reader uses. It reads three things and nothing more:
//!
//! 1. **The arrow schema** — to confirm the three structurally-required columns
//!    `basin_id`, `delineation`, `geometry` are present (Geo1). A missing one is a
//!    typed [`CoreError::MissingGeometryColumn`], never a panic.
//! 2. **The `delineation` and `basin_id` columns** — a **bounded 1-D key-column
//!    read** (the exact pattern the scalar reader uses for `basin_id`/`time`): each
//!    column is projected **by name alone**, so the `geometry` blob is **never
//!    decoded** (architecture §1). The distinct `delineation` labels become a
//!    [`DelineationLabel`] list (spec §9 — opaque producer strings, no "hydrofabric"
//!    assumption); the distinct `basin_id` values become a [`BasinId`] list (the I1
//!    input).
//! 3. **The `geo` key-value metadata** — to recover the CRS. The geoparquet `geo`
//!    block stores the primary geometry column's CRS as a **PROJJSON object**. HDX
//!    records the CRS as a *comparable* [`Crs`] per the CRS-recording rule
//!    (architecture amendment): when the PROJJSON carries an `id` with
//!    `authority == "EPSG"` and a numeric/string `code`, the reader records
//!    `Crs::new("EPSG:<code>")` — a value MS6's M5 can compare to the manifest's
//!    `"EPSG:4326"`. When there is no resolvable EPSG `id`, the reader records the
//!    raw PROJJSON string verbatim and flags the file as an **R3** item via
//!    [`CrsSource::RawProjjsonR3`] (documented, never silently claimed).
//!
//! ## "Not partitioned by delineation" (Geo1)
//!
//! `outlines.geoparquet` is a **single file at the dataset root** — *not* a
//! `delineation=<label>/` hive (spec §9: the `delineation` column distinguishes the
//! delineations *within one file*). [`read_outlines`] reads one file, so it records
//! [`OutlinesInfo::partitioned_by_delineation`] as `false`: a structural fact about
//! the artifact this reader was handed, never a decision.
//!
//! ## Inert / agnostic (spec §1)
//!
//! Every datum here is a structural fact: column-presence booleans, a delineation
//! label list, a basin-id list, a recorded CRS string, and which path resolved it.
//! No type or field carries transform, role, semantic type, or provenance. The
//! `geometry` payload is never decoded.

use std::fs;
use std::path::Path;

use arrow::array::Array;
use arrow::datatypes::Schema;
use bytes::Bytes;
use parquet::arrow::ProjectionMask;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use parquet::file::metadata::ParquetMetaData;
use serde_json::Value;
use tracing::{debug, info, instrument};

use crate::error::CoreError;
use crate::newtypes::{BasinId, Crs, DelineationLabel};
use crate::parquet_meta::read_parquet_meta;

/// The authoritative in-file basin-id column of the outlines table (spec §3/§14 I1).
const BASIN_ID_COLUMN: &str = "basin_id";
/// The neutral delineation-label column (spec §9/§14 Geo1).
const DELINEATION_COLUMN: &str = "delineation";
/// The geometry blob column — present in the schema, **never decoded** (spec §9).
const GEOMETRY_COLUMN: &str = "geometry";

/// The geoparquet footer key carrying the dataset `geo` metadata (geoparquet spec).
const GEO_KV_KEY: &str = "geo";
/// The EPSG authority string a PROJJSON `id.authority` must equal for an EPSG resolve.
const EPSG_AUTHORITY: &str = "EPSG";

/// How [`OutlinesInfo::crs`] was resolved (spec §7/§11, the M5-readiness fact).
///
/// An enum, never a `bool`, so the resolution path is self-documenting at every call
/// site (architecture §3.3). MS6's M5 cross-check uses this to know whether the
/// recorded [`Crs`] is a *comparable* `EPSG:<code>` (compare it to the manifest) or a
/// raw PROJJSON string that needs an R3 byte-deep follow-up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CrsSource {
    /// The CRS was resolved to `EPSG:<code>` from the PROJJSON `id`
    /// (`id.authority == "EPSG"`). The recorded [`Crs`] is a **comparable** value
    /// MS6's M5 can compare directly to the manifest's `"EPSG:4326"` (the MEDIUM fix).
    EpsgFromProjjsonId,
    /// The PROJJSON carried no resolvable EPSG `id` (no `id`, or
    /// `authority != "EPSG"`), so the reader recorded the **raw PROJJSON string
    /// verbatim** and flags this file as an **R3** item — its M5 readiness needs a
    /// byte-deep follow-up (documented, never silently claimed).
    RawProjjsonR3,
}

/// The discovered facts of `outlines.geoparquet` (spec §9, feeds §14 Geo1/I1/M5).
///
/// Holds exactly the structural facts the geometry-side checks need: which of the
/// three required columns are present (Geo1), the distinct delineation labels (spec
/// §9), whether the in-file `basin_id` column is present plus its distinct values
/// (the I1 input), whether the artifact is partitioned by delineation (always `false`
/// for the single root file), and the recorded [`Crs`] plus the [`CrsSource`] that
/// resolved it (the M5-readiness fact). It records facts; it enforces nothing.
///
/// Inert / agnostic (spec §1): presence booleans, label/id lists, a CRS string, and
/// the path that resolved it — no transform/role/semantic/provenance, no geometry.
#[derive(Debug, Clone, PartialEq)]
pub struct OutlinesInfo {
    delineations: Vec<DelineationLabel>,
    basin_ids: Vec<BasinId>,
    has_basin_id: bool,
    has_delineation: bool,
    has_geometry: bool,
    partitioned_by_delineation: bool,
    crs: Crs,
    crs_source: CrsSource,
}

impl OutlinesInfo {
    /// Borrows the distinct delineation labels read from the `delineation` column
    /// (spec §9). Order is first-seen; the labels are opaque producer strings.
    pub fn delineations(&self) -> &[DelineationLabel] {
        &self.delineations
    }

    /// Borrows the distinct in-file `basin_id` values read from the bounded
    /// key-column read (the I1 input). Empty when `basin_id` is absent.
    pub fn basin_ids(&self) -> &[BasinId] {
        &self.basin_ids
    }

    /// Returns `true` iff the `basin_id` column is present (spec §3/§14 I1).
    pub fn has_basin_id(&self) -> bool {
        self.has_basin_id
    }

    /// Returns `true` iff the `delineation` column is present (spec §9/§14 Geo1).
    pub fn has_delineation(&self) -> bool {
        self.has_delineation
    }

    /// Returns `true` iff the `geometry` column is present (spec §9/§14 Geo1). The
    /// blob is never decoded — only its presence in the schema is recorded.
    pub fn has_geometry(&self) -> bool {
        self.has_geometry
    }

    /// Returns `true` iff the artifact is partitioned by delineation (spec §9).
    ///
    /// Always `false` for the single root `outlines.geoparquet` this reader is handed
    /// — a structural fact about the artifact, not a decision the reader makes.
    pub fn partitioned_by_delineation(&self) -> bool {
        self.partitioned_by_delineation
    }

    /// Borrows the recorded CRS (the comparable `EPSG:<code>` or the raw PROJJSON).
    pub fn crs(&self) -> &Crs {
        &self.crs
    }

    /// Returns how the CRS was resolved — the M5-readiness fact (spec §7/§11).
    pub fn crs_source(&self) -> &CrsSource {
        &self.crs_source
    }
}

/// Reads `outlines.geoparquet` into an [`OutlinesInfo`] (spec §9, feeds Geo1/I1/M5).
///
/// Reuses the private `parquet_meta` touchpoint for the schema + the `geo` key-value
/// metadata, then a **bounded 1-D read** of the `delineation` and `basin_id` columns
/// (each projected by name alone — the `geometry` blob is never decoded,
/// architecture §1). Records the column-presence facts (Geo1), the distinct
/// delineation labels (spec §9), the distinct `basin_id` values (I1), the
/// not-partitioned fact (single root file), and the CRS resolved from the PROJJSON
/// `id` (the MEDIUM fix) or recorded raw with an R3 flag.
///
/// # Errors
///
/// | Condition | Error |
/// |---|---|
/// | the file cannot be read, or its parquet metadata fails to decode | [`CoreError::GeoparquetRead`] |
/// | any of `basin_id` / `delineation` / `geometry` is absent from the schema | [`CoreError::MissingGeometryColumn`] |
/// | the `geo` KV is absent / malformed, or its primary column's CRS is unreadable | [`CoreError::GeoparquetRead`] |
/// | a bounded `delineation` / `basin_id` column is not a string array | [`CoreError::GeoparquetRead`] |
#[instrument(fields(path = %path.as_ref().display()))]
pub fn read_outlines(path: impl AsRef<Path>) -> Result<OutlinesInfo, CoreError> {
    let path = path.as_ref();
    let artifact = path.display().to_string();
    let bytes = read_file_bytes(&artifact, path)?;

    // Metadata only first (architecture §1): the arrow schema + the `geo` KV. No
    // chunk and no `geometry` blob are decoded here.
    let meta =
        read_parquet_meta(&artifact, &bytes).map_err(|e| reread_as_geoparquet(&artifact, e))?;
    let schema = meta.schema();

    // Geo1: the three required columns must be present.
    require_column(&artifact, schema, BASIN_ID_COLUMN)?;
    require_column(&artifact, schema, DELINEATION_COLUMN)?;
    require_column(&artifact, schema, GEOMETRY_COLUMN)?;

    // The CRS from the `geo` KV primary-column PROJJSON (the MEDIUM fix).
    let geo = meta
        .key_value(GEO_KV_KEY)
        .ok_or_else(|| CoreError::GeoparquetRead {
            artifact: artifact.clone(),
            detail: "no `geo` key-value metadata in the parquet footer".to_string(),
        })?;
    let (crs, crs_source) = resolve_crs(&artifact, geo)?;

    // Bounded 1-D key-column reads: the `geometry` blob is never projected.
    let metadata = meta.file_metadata();
    let delineation_strings =
        read_string_column(&artifact, bytes.clone(), metadata, DELINEATION_COLUMN)?;
    let delineations = distinct(delineation_strings.into_iter().map(DelineationLabel::new));

    let basin_id_strings = read_string_column(&artifact, bytes, metadata, BASIN_ID_COLUMN)?;
    let basin_ids = distinct(basin_id_strings.into_iter().map(BasinId::new));

    info!(
        delineations = delineations.len(),
        basin_ids = basin_ids.len(),
        crs = crs.as_str(),
        crs_source = ?crs_source,
        "read outlines geoparquet metadata"
    );

    Ok(OutlinesInfo {
        delineations,
        basin_ids,
        has_basin_id: true,
        has_delineation: true,
        has_geometry: true,
        // A single root `outlines.geoparquet` is never a `delineation=<x>/` hive.
        partitioned_by_delineation: false,
        crs,
        crs_source,
    })
}

/// Reads the file at `path` into bytes, mapping IO failure to a typed error.
///
/// # Errors
///
/// | Condition | Error |
/// |---|---|
/// | the file cannot be read (missing / not readable) | [`CoreError::GeoparquetRead`] |
fn read_file_bytes(artifact: &str, path: &Path) -> Result<Bytes, CoreError> {
    fs::read(path)
        .map(Bytes::from)
        .map_err(|e| CoreError::GeoparquetRead {
            artifact: artifact.to_string(),
            detail: e.to_string(),
        })
}

/// Re-labels a `parquet_meta` [`CoreError::ParquetRead`] as a geoparquet read error.
///
/// The shared touchpoint reports a generic [`CoreError::ParquetRead`]; the outlines
/// reader surfaces its own [`CoreError::GeoparquetRead`] so a caller maps the failure
/// back to the spec §9 artifact. Any other error variant is passed through unchanged.
fn reread_as_geoparquet(artifact: &str, error: CoreError) -> CoreError {
    match error {
        CoreError::ParquetRead { detail, .. } => CoreError::GeoparquetRead {
            artifact: artifact.to_string(),
            detail,
        },
        other => other,
    }
}

/// Confirms the schema carries `column`, else fires [`CoreError::MissingGeometryColumn`].
///
/// # Errors
///
/// | Condition | Error |
/// |---|---|
/// | `column` is absent from the arrow schema | [`CoreError::MissingGeometryColumn`] |
fn require_column(artifact: &str, schema: &Schema, column: &str) -> Result<(), CoreError> {
    if schema.fields().iter().any(|f| f.name() == column) {
        Ok(())
    } else {
        Err(CoreError::MissingGeometryColumn {
            artifact: artifact.to_string(),
            column: column.to_string(),
        })
    }
}

/// Resolves the recorded CRS from the geoparquet `geo` KV JSON (the MEDIUM fix).
///
/// Parses the `geo` JSON, takes `primary_column`'s `crs` (a PROJJSON object), and
/// records [`Crs::new`]`("EPSG:<code>")` + [`CrsSource::EpsgFromProjjsonId`] when the
/// PROJJSON carries an `id` with `authority == "EPSG"` and a code. Otherwise it
/// records the **raw PROJJSON string verbatim** + [`CrsSource::RawProjjsonR3`].
///
/// # Errors
///
/// | Condition | Error |
/// |---|---|
/// | the `geo` value is not valid JSON | [`CoreError::GeoparquetRead`] |
/// | `geo` carries no `primary_column`, or that column / its `crs` is absent | [`CoreError::GeoparquetRead`] |
fn resolve_crs(artifact: &str, geo: &str) -> Result<(Crs, CrsSource), CoreError> {
    let geo_json: Value = serde_json::from_str(geo).map_err(|e| CoreError::GeoparquetRead {
        artifact: artifact.to_string(),
        detail: format!("`geo` metadata is not valid JSON: {e}"),
    })?;

    let primary = geo_json
        .get("primary_column")
        .and_then(Value::as_str)
        .ok_or_else(|| CoreError::GeoparquetRead {
            artifact: artifact.to_string(),
            detail: "`geo` metadata has no string `primary_column`".to_string(),
        })?;

    let crs_json = geo_json
        .get("columns")
        .and_then(|c| c.get(primary))
        .and_then(|col| col.get("crs"))
        .ok_or_else(|| CoreError::GeoparquetRead {
            artifact: artifact.to_string(),
            detail: format!("`geo` metadata has no CRS for primary column {primary:?}"),
        })?;

    // The MEDIUM fix: prefer the comparable EPSG:<code> from the PROJJSON `id`.
    match epsg_code_from_projjson(crs_json) {
        Some(code) => {
            debug!(epsg = %code, "resolved geoparquet CRS from PROJJSON id");
            Ok((
                Crs::new(format!("{EPSG_AUTHORITY}:{code}")),
                CrsSource::EpsgFromProjjsonId,
            ))
        }
        None => {
            // R3 fallback (documented): record the raw PROJJSON string verbatim.
            debug!("geoparquet CRS PROJJSON has no EPSG id; recording raw (R3)");
            Ok((Crs::new(crs_json.to_string()), CrsSource::RawProjjsonR3))
        }
    }
}

/// Extracts the EPSG code from a PROJJSON `crs` object's `id`, if it is an EPSG id.
///
/// Returns `Some(code)` only when `id.authority == "EPSG"` and `id.code` is present
/// (a JSON number, e.g. `4326`, or a string). The code is rendered as a plain
/// integer string (`4326`) when it is an integer number, else as its string form, so
/// the recorded CRS reads `EPSG:4326` rather than `EPSG:4326.0`. Returns `None` for a
/// PROJJSON without an `id`, or with a non-EPSG authority — the R3 fallback case.
fn epsg_code_from_projjson(crs_json: &Value) -> Option<String> {
    let id = crs_json.get("id")?;
    let authority = id.get("authority").and_then(Value::as_str)?;
    if authority != EPSG_AUTHORITY {
        return None;
    }
    match id.get("code")? {
        Value::Number(n) if n.is_u64() => n.as_u64().map(|c| c.to_string()),
        Value::Number(n) if n.is_i64() => n.as_i64().map(|c| c.to_string()),
        Value::Number(n) => Some(n.to_string()),
        Value::String(s) => Some(s.clone()),
        _ => None,
    }
}

/// Reads one string column by name into its values, via a bounded 1-D projection.
///
/// Projects **exactly the one named column** ([`ProjectionMask::leaves`] over a
/// single leaf index), so no other column — and never the `geometry` blob — is
/// touched (architecture §1: a 1-D key-column read). Values are returned in file
/// order, nulls skipped.
///
/// # Errors
///
/// | Condition | Error |
/// |---|---|
/// | the column is absent from the parquet schema | [`CoreError::MissingGeometryColumn`] |
/// | the parquet reader fails, or the column is not a string array | [`CoreError::GeoparquetRead`] |
fn read_string_column(
    artifact: &str,
    bytes: Bytes,
    metadata: &ParquetMetaData,
    column: &str,
) -> Result<Vec<String>, CoreError> {
    let col_idx =
        leaf_column_index(metadata, column).ok_or_else(|| CoreError::MissingGeometryColumn {
            artifact: artifact.to_string(),
            column: column.to_string(),
        })?;

    let builder =
        ParquetRecordBatchReaderBuilder::try_new(bytes).map_err(|e| CoreError::GeoparquetRead {
            artifact: artifact.to_string(),
            detail: e.to_string(),
        })?;
    let mask = ProjectionMask::leaves(builder.parquet_schema(), [col_idx]);
    let reader = builder
        .with_projection(mask)
        .build()
        .map_err(|e| CoreError::GeoparquetRead {
            artifact: artifact.to_string(),
            detail: e.to_string(),
        })?;

    debug!(
        column,
        leaf = col_idx,
        "bounded 1-D outlines key-column read"
    );

    let mut values: Vec<String> = Vec::new();
    for batch in reader {
        let batch = batch.map_err(|e| CoreError::GeoparquetRead {
            artifact: artifact.to_string(),
            detail: e.to_string(),
        })?;
        let array = batch.column(0);
        values.extend(string_values(artifact, column, array.as_ref())?);
    }
    Ok(values)
}

/// Locates a leaf column index in the parquet schema by name, or `None`.
fn leaf_column_index(metadata: &ParquetMetaData, name: &str) -> Option<usize> {
    metadata
        .file_metadata()
        .schema_descr()
        .columns()
        .iter()
        .position(|c| c.name() == name)
}

/// Extracts the string values of a `Utf8` / `LargeUtf8` arrow array, skipping nulls.
///
/// # Errors
///
/// | Condition | Error |
/// |---|---|
/// | the array is neither `Utf8` nor `LargeUtf8` | [`CoreError::GeoparquetRead`] (an unexpected column encoding) |
fn string_values(
    artifact: &str,
    column: &str,
    array: &dyn Array,
) -> Result<Vec<String>, CoreError> {
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
        Err(CoreError::GeoparquetRead {
            artifact: artifact.to_string(),
            detail: format!("column {column:?} is not a string array"),
        })
    }
}

/// Collects items into a `Vec`, dropping later duplicates (first-seen order kept).
fn distinct<T: PartialEq>(items: impl Iterator<Item = T>) -> Vec<T> {
    let mut seen: Vec<T> = Vec::new();
    for item in items {
        if !seen.contains(&item) {
            seen.push(item);
        }
    }
    seen
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use arrow::array::StringArray;
    use arrow::datatypes::{DataType, Field as ArrowField, Schema};
    use arrow::record_batch::RecordBatch;
    use parquet::arrow::ArrowWriter;
    use parquet::file::metadata::KeyValue;
    use parquet::file::properties::WriterProperties;

    use crate::error::CoreError;
    use crate::geoparquet_reader::{CrsSource, read_outlines};
    use crate::newtypes::{BasinId, Crs, DelineationLabel};

    /// Resolves a path under the committed `conformance/` fixture tree.
    ///
    /// `CARGO_MANIFEST_DIR` is `crates/core`; the fixtures live two levels up.
    fn conformance(rel: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../conformance")
            .join(rel)
    }

    /// The committed valid-fixture outlines artifact.
    fn fixture_outlines() -> PathBuf {
        conformance("valid/minimal/outlines.geoparquet")
    }

    /// Writes a synthetic outlines-style geoparquet to memory, with control over the
    /// columns present and the `geo` KV metadata (for the unit / negative tests).
    ///
    /// `geo` is attached as a parquet footer **key-value metadata** entry (the
    /// standalone `geo` key the geoparquet spec mandates), via
    /// [`WriterProperties::set_key_value_metadata`] — the same footer slot a
    /// geoparquet writer uses — so the reader's `geo` recovery is exercised against a
    /// real footer entry, not the serialized arrow-schema blob.
    fn write_outlines(columns: &[&str], geo: Option<&str>) -> Vec<u8> {
        let arrow_fields: Vec<ArrowField> = columns
            .iter()
            .map(|c| ArrowField::new(*c, DataType::Utf8, false))
            .collect();
        let schema = Arc::new(Schema::new(arrow_fields));

        // Two rows; every column gets the same two placeholder values (the reader
        // dedups by value, never decoding meaning).
        let arrays: Vec<Arc<dyn arrow::array::Array>> = columns
            .iter()
            .map(|_| Arc::new(StringArray::from(vec!["a", "b"])) as Arc<dyn arrow::array::Array>)
            .collect();
        let batch =
            RecordBatch::try_new(Arc::clone(&schema), arrays).expect("synthetic batch must build");

        let props = geo.map(|g| {
            WriterProperties::builder()
                .set_key_value_metadata(Some(vec![KeyValue::new("geo".to_string(), g.to_string())]))
                .build()
        });

        let mut buffer: Vec<u8> = Vec::new();
        {
            let mut writer = ArrowWriter::try_new(&mut buffer, schema, props)
                .expect("arrow parquet writer must construct");
            writer.write(&batch).expect("write must succeed");
            writer.close().expect("close must succeed");
        }
        buffer
    }

    /// Writes a synthetic geoparquet to a temp file and returns its path.
    fn write_outlines_tempfile(columns: &[&str], geo: Option<&str>) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        // A process-wide counter guarantees a unique name even when two calls land
        // in the same nanosecond under parallel test execution; pid + nanos alone
        // collide (the test suite ran flaky without this).
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let bytes = write_outlines(columns, geo);
        let mut path = std::env::temp_dir();
        let unique = format!(
            "hdx_outlines_{}_{}_{}.geoparquet",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        path.push(unique);
        std::fs::write(&path, bytes).expect("writing the temp geoparquet must succeed");
        path
    }

    /// A minimal PROJJSON `geo` block with an EPSG `id` (the fixture shape).
    fn geo_with_epsg_id() -> String {
        r#"{
            "version": "1.0.0",
            "primary_column": "geometry",
            "columns": {
                "geometry": {
                    "encoding": "WKB",
                    "crs": {
                        "type": "GeographicCRS",
                        "name": "WGS 84",
                        "id": { "authority": "EPSG", "code": 4326 }
                    }
                }
            }
        }"#
        .to_string()
    }

    /// A PROJJSON `geo` block whose CRS carries **no `id`** (the R3 fallback case).
    fn geo_without_id() -> String {
        r#"{
            "version": "1.0.0",
            "primary_column": "geometry",
            "columns": {
                "geometry": {
                    "encoding": "WKB",
                    "crs": {
                        "type": "GeographicCRS",
                        "name": "WGS 84"
                    }
                }
            }
        }"#
        .to_string()
    }

    #[test]
    fn geo1_schema_present_and_not_partitioned() {
        // Geo1: the three required columns are present, and the single root file is
        // never partitioned by delineation.
        let info = read_outlines(fixture_outlines())
            .expect("the valid fixture outlines must read without error");

        assert!(info.has_basin_id(), "basin_id column present (I1)");
        assert!(info.has_delineation(), "delineation column present (Geo1)");
        assert!(info.has_geometry(), "geometry column present (Geo1)");
        assert!(
            !info.partitioned_by_delineation(),
            "a single root outlines.geoparquet is not partitioned by delineation (Geo1)"
        );
    }

    #[test]
    fn delineation_labels_are_grit_and_merit_order_insensitive() {
        let info = read_outlines(fixture_outlines()).expect("fixture must read");

        // `{grit, merit}`, order-insensitive — each an opaque DelineationLabel.
        let mut got: Vec<&str> = info
            .delineations()
            .iter()
            .map(DelineationLabel::as_str)
            .collect();
        got.sort_unstable();
        assert_eq!(got, vec!["grit", "merit"]);
    }

    #[test]
    fn i1_input_has_basin_id_and_distinct_values() {
        let info = read_outlines(fixture_outlines()).expect("fixture must read");

        assert!(
            info.has_basin_id(),
            "I1: outlines carries a basin_id column"
        );

        // The bounded basin_id read returns the distinct ids {0001, 0002, 0003}.
        let mut ids: Vec<&str> = info.basin_ids().iter().map(BasinId::as_str).collect();
        ids.sort_unstable();
        assert_eq!(ids, vec!["0001", "0002", "0003"]);

        // §9 plurality: there are 4 rows and basin 0001 carries BOTH delineations,
        // so the distinct basin set (3) is smaller than the row count (4) — the
        // multiplicity is the plurality signal. The distinct delineations are both
        // labels, and basin 0001 appearing under each is what makes the 4th row.
        assert_eq!(
            info.basin_ids().len(),
            3,
            "three distinct basins across four rows (basin 0001 carries both delineations)"
        );
        assert_eq!(
            info.delineations().len(),
            2,
            "two delineations: grit + merit"
        );
    }

    #[test]
    fn crs_resolves_to_epsg_4326_from_projjson_id() {
        // The MEDIUM fix asserted on the fixture: the reader resolves the comparable
        // EPSG:4326 from the PROJJSON `id`, NOT the raw PROJJSON blob — so MS6's M5
        // receives a value comparable to the manifest's "EPSG:4326".
        let info = read_outlines(fixture_outlines()).expect("fixture must read");

        assert_eq!(info.crs(), &Crs::new("EPSG:4326"));
        assert_eq!(info.crs_source(), &CrsSource::EpsgFromProjjsonId);
    }

    #[test]
    fn crs_without_id_records_raw_projjson_with_r3_flag() {
        // The documented R3 fallback: a PROJJSON without an `id` is recorded raw +
        // CrsSource::RawProjjsonR3 — never a panic, never a silent EPSG claim.
        let path = write_outlines_tempfile(
            &["basin_id", "delineation", "geometry"],
            Some(&geo_without_id()),
        );
        let info = read_outlines(&path).expect("a synthetic outlines without an id must read");
        let _ = std::fs::remove_file(&path);

        assert_eq!(info.crs_source(), &CrsSource::RawProjjsonR3);
        // The raw PROJJSON string is recorded verbatim (it is a JSON object, not an
        // `EPSG:` string), so the CRS is clearly not a comparable EPSG value.
        assert!(
            info.crs().as_str().contains("GeographicCRS"),
            "the raw PROJJSON is recorded verbatim"
        );
        assert_ne!(info.crs(), &Crs::new("EPSG:4326"));
    }

    #[test]
    fn missing_delineation_column_is_missing_geometry_column_error() {
        // Negative: an outlines parquet missing the `delineation` column → the typed
        // MissingGeometryColumn error (Geo1), never a panic.
        let path = write_outlines_tempfile(&["basin_id", "geometry"], Some(&geo_with_epsg_id()));
        let result = read_outlines(&path);
        let _ = std::fs::remove_file(&path);

        match result {
            Err(CoreError::MissingGeometryColumn { artifact, column }) => {
                assert!(!artifact.is_empty());
                assert_eq!(column, "delineation");
            }
            other => {
                panic!("expected MissingGeometryColumn for the missing delineation, got {other:?}")
            }
        }
    }

    #[test]
    fn epsg_id_unit_proves_comparability_on_a_synthetic_geo() {
        // A focused unit on the EPSG-from-id path independent of the fixture: a
        // synthetic geo with an EPSG id resolves to the comparable EPSG:4326.
        let path = write_outlines_tempfile(
            &["basin_id", "delineation", "geometry"],
            Some(&geo_with_epsg_id()),
        );
        let info = read_outlines(&path).expect("synthetic outlines with an EPSG id must read");
        let _ = std::fs::remove_file(&path);

        assert_eq!(info.crs(), &Crs::new("EPSG:4326"));
        assert_eq!(info.crs_source(), &CrsSource::EpsgFromProjjsonId);
    }
}
