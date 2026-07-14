//! The Zarr v3 **metadata** reader for `gridded_dynamic/<label>.zarr` stores
//! (spec §7/§8, architecture §1/§3.5).
//!
//! This module reads the *shape* of a per-basin Zarr v3 store into typed facts —
//! the gridded·dynamic field catalog plus a [`GridInfo`] — **never** its scientific
//! grid values. It reads exactly two tiers (architecture §1):
//!
//! 1. the store's **root `zarr.json`**, once, via the §8 **consolidated-metadata**
//!    path (`consolidated_metadata.metadata`, `kind == "inline"`), which carries the
//!    metadata of *every* member in one object; and
//! 2. the **1-D `lat`/`lon`/`time` coordinate chunks** (`c/0`), a 1-D coordinate
//!    read (architecture §1) used to derive the grid extent.
//!
//! It **never** opens a `c/0/0/0` data chunk: the sharded data arrays
//! (`era5_precipitation`, `era5_precipitation_was_filled`) are read for metadata
//! only and their chunk payloads are opaque leaves.
//!
//! ## The consolidated-metadata path (spec §8)
//!
//! The reader learns the whole store from **one read** of the root `zarr.json`'s
//! `consolidated_metadata.metadata` map (the live fixture has `kind == "inline"` and
//! six members: `crs`, `era5_precipitation`, `era5_precipitation_was_filled`, `lat`,
//! `lon`, `time`). Which path was taken is recorded in a self-documenting
//! [`ConsolidatedMetadataSource`] enum — never a `bool`. If the store does not
//! expose inline consolidated metadata, the source is recorded as
//! [`ConsolidatedMetadataSource::R3Skip`] with a stated reason — documented, never
//! silently claimed. A zarr-python-vs-Rust mismatch is fixed by **regenerating the
//! fixture**, never a reader workaround.
//!
//! The typed per-member array metadata is parsed with `zarrs_metadata` (the
//! metadata-only sub-crate of `zarrs`), whose `v3::ArrayMetadataV3` models a Zarr v3
//! array's `shape` / `data_type` / `attributes` / `dimension_names`. The
//! consolidated map itself is read once with `serde_json` and each entry
//! deserialized into `ArrayMetadataV3`. The 1-D coordinate chunks are decompressed
//! with the pure-Rust `ruzstd` decoder. **No GDAL, no async, no cloud, no chunk-IO
//! crate.**
//!
//! ## Array classification
//!
//! - A **coordinate array** is one named `time`/`lat`/`lon` that self-references its
//!   own dimension via `dimension_names == [name]`.
//! - A **data variable** (a gridded·dynamic [`Field`]) is an array carrying a CF
//!   `grid_mapping` attribute and 3-D `dimension_names` `[time, lat, lon]`.
//! - The **`grid_mapping` target** (here `crs`) is resolved **exclusively by
//!   following a data variable's `grid_mapping` attribute** — *not* by dimension
//!   self-reference. The `crs` array has `shape: []` and no `dimension_names`, so it
//!   is unreachable by dimension grouping; it is read only after a data var points
//!   at it.
//!
//! Each data variable becomes one ordinary [`Field`] named **exactly** as the Zarr
//! variable, with no name-pattern special-casing: `era5_precipitation_was_filled` is
//! an ordinary `GriddedDynamic` field, not a companion mask (spec §1/§2). Like the
//! scalar reader, this is a discovery surface — it records facts and enforces no spec
//! §14 check.
//!
//! ## Glossary
//!
//! | Term | Meaning |
//! |---|---|
//! | consolidated metadata | the §8 inline map in the root `zarr.json` carrying every member's metadata (one read) |
//! | coordinate array | a 1-D `time`/`lat`/`lon` array self-referencing its dimension (CF cell centers) |
//! | data variable | a gridded·dynamic array with `grid_mapping` + 3-D `[time, lat, lon]` dims |
//! | grid_mapping target | the `crs` array a data var's `grid_mapping` attribute names |
//! | center→edge | the half-pixel conversion from CF cell centers to the cell-edge extent |

use std::io::Read;
use std::path::Path;

use ruzstd::StreamingDecoder;
use serde_json::Value;
use tracing::{debug, info, instrument, warn};
use zarrs_metadata::v3::ArrayMetadataV3;

use crate::error::CoreError;
use crate::field::{Dtype, Field, Quadrant, Units, parse_dtype};
use crate::grid::{GridExtent, GridInfo, GridResolution, center_to_edge};
use crate::newtypes::{Crs, FieldName, GridLabel};

/// The three coordinate-array names the CF convention mandates (spec §7.3).
const COORD_TIME: &str = "time";
/// The latitude coordinate-array name (spec §7.3).
const COORD_LAT: &str = "lat";
/// The longitude coordinate-array name (spec §7.3).
const COORD_LON: &str = "lon";

/// The CF attribute on a data variable naming its `grid_mapping` target (spec §7.3).
const GRID_MAPPING_ATTR: &str = "grid_mapping";
/// The optional CF standard-name attribute carried by a data variable.
const STANDARD_NAME_ATTR: &str = "standard_name";
/// The CF attribute carrying a variable's units (spec §2).
const UNITS_ATTR: &str = "units";

/// The number of microseconds in one whole day (`86_400_000_000`).
///
/// The gridded·dynamic Zarr `time` coordinate is stored as int64 **days** (units
/// `days since 1970-01-01`); the scalar/reduced axis HDX compares against is i64
/// **microseconds** since the unix epoch. This is the exact, integral day→micros
/// scale ([`normalize_days_to_micros`]) — it mirrors the producer's own conversion
/// (orthographos `axis.rs` / `exec.rs`) so the two axes are bit-comparable.
pub(crate) const MICROS_PER_DAY: i64 = 86_400_000_000;

/// Which path the reader used to learn the store (spec §8).
///
/// An enum, never a `bool`, so the path taken is self-documenting at every call site
/// (architecture §3.3). Downstream consumers can report which path produced the
/// model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConsolidatedMetadataSource {
    /// The store was learned from **one read** of the root `zarr.json`'s inline
    /// `consolidated_metadata.metadata` map (the §8 consolidated path — the live
    /// path on a conformant fixture). Records the enumerated member names so the
    /// "all members from one read" claim is checkable.
    Consolidated {
        /// The names of every member enumerated from the single consolidated read,
        /// in the order they appear in the map.
        members: Vec<String>,
    },
    /// Consolidated metadata could not be used: the root `zarr.json` carries no
    /// `consolidated_metadata` object, or its `kind` is not `"inline"`. Recorded
    /// with a stated reason as a byte-deep skip — documented, never silently claimed.
    R3Skip {
        /// Why the consolidated path was unavailable; opaque, for honest reporting.
        reason: String,
    },
}

/// The discovered facts of one `gridded_dynamic/<label>.zarr` store (spec §7/§8).
///
/// Holds the gridded·dynamic [`Field`] catalog (one ordinary field per data
/// variable), the per-store [`GridInfo`] (the center→edge extent + signed
/// resolution + dims + recorded CRS), and which [`ConsolidatedMetadataSource`] path
/// the reader used. It records facts; it enforces nothing.
///
/// Inert / agnostic (spec §1): a field list, a grid geometry, a CRS string, and the
/// path taken — no transform/role/semantic/provenance.
#[derive(Debug, Clone, PartialEq)]
pub struct ZarrGrid {
    grid_info: GridInfo,
    fields: Vec<Field>,
    consolidated_source: ConsolidatedMetadataSource,
    gridded_time_micros: Vec<i64>,
}

impl ZarrGrid {
    /// Borrows the per-store grid geometry (extent / resolution / dims / CRS).
    pub fn grid_info(&self) -> &GridInfo {
        &self.grid_info
    }

    /// Borrows the gridded·dynamic field catalog (one ordinary field per data var).
    pub fn fields(&self) -> &[Field] {
        &self.fields
    }

    /// Borrows the consolidated-metadata path the reader took.
    pub fn consolidated_source(&self) -> &ConsolidatedMetadataSource {
        &self.consolidated_source
    }

    /// Borrows the store's `time` coordinate as i64 **microseconds** since the unix
    /// epoch (spec §6.2/§6.3).
    ///
    /// The Zarr `time` coordinate is stored as int64 **days** (`days since
    /// 1970-01-01`); this is that array decoded via [`read_coord_i64`] and normalized
    /// through [`normalize_days_to_micros`] — the SAME i64-micros representation the
    /// scalar/reduced axis HDX compares against (so `check_t2`/`check_m6(b)` compare
    /// the two axes bit-for-bit on one representation). An inert fact: discovery
    /// surfaces the axis; it renders no verdict.
    pub fn gridded_time_micros(&self) -> &[i64] {
        &self.gridded_time_micros
    }
}

/// Bridges a Zarr v3 `data_type` string to the canonical dtype string
/// [`parse_dtype`] accepts (the single documented Zarr→[`Dtype`] map).
///
/// This mirrors the scalar reader's arrow-type bridge: it maps the *physical*
/// element encodings a Zarr v3 store declares to HDX's closed [`Dtype`] set, and
/// returns `None` for anything else so the caller surfaces a typed
/// [`CoreError::UnknownDtype`] rather than inventing a mapping. It interprets no
/// semantics (spec §1) — it records *how a value is physically encoded*.
///
/// | Zarr `data_type` | canonical string |
/// |---|---|
/// | `float32` | `f32` |
/// | `float64` | `f64` |
/// | `int32` | `i32` |
/// | `int64` | `i64` |
/// | `bool` | `bool` |
/// | `int8` / `uint8` | `bool` (a 1-byte integer is Zarr/NumPy's physical encoding of a 0/1 mask, which has no narrower closed-set member) |
///
/// `int8`/`uint8` map to `bool`: the closed [`Dtype`] set has no 8-bit integer, and
/// a 1-byte integer array is the physical encoding a producer uses for a 0/1 mask
/// (spec §12 companion masks). This is a *physical-encoding* bridge, not a semantic
/// one — no name is inspected.
fn zarr_dtype_str(data_type: &str) -> Option<&'static str> {
    match data_type {
        "float32" => Some("f32"),
        "float64" => Some("f64"),
        "int32" => Some("i32"),
        "int64" => Some("i64"),
        "bool" => Some("bool"),
        "int8" | "uint8" => Some("bool"),
        _ => None,
    }
}

/// Parses a Zarr v3 `data_type` string into a closed [`Dtype`] at the boundary.
///
/// # Errors
///
/// | Condition | Error |
/// |---|---|
/// | the Zarr `data_type` does not map to a supported encoding (see [`zarr_dtype_str`]) | [`CoreError::UnknownDtype`] (with `found` echoing the Zarr string) |
fn zarr_dtype(data_type: &str) -> Result<Dtype, CoreError> {
    match zarr_dtype_str(data_type) {
        Some(s) => parse_dtype(s),
        None => Err(CoreError::UnknownDtype {
            found: data_type.to_string(),
        }),
    }
}

/// Reads a string attribute from a Zarr array's `attributes` map, if present.
fn string_attr(meta: &ArrayMetadataV3, key: &str) -> Option<String> {
    meta.attributes
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// Returns `true` iff this array self-references the single dimension `name` —
/// i.e. its `dimension_names` is exactly `[Some(name)]` (a 1-D coordinate array).
fn self_references_dimension(meta: &ArrayMetadataV3, name: &str) -> bool {
    match &meta.dimension_names {
        Some(dims) => dims.len() == 1 && dims[0].as_deref() == Some(name),
        None => false,
    }
}

/// Returns `true` iff this array's `dimension_names` is exactly `[time, lat, lon]`
/// (a 3-D gridded·dynamic data variable).
fn is_three_dim_grid(meta: &ArrayMetadataV3) -> bool {
    match &meta.dimension_names {
        Some(dims) => {
            dims.len() == 3
                && dims[0].as_deref() == Some(COORD_TIME)
                && dims[1].as_deref() == Some(COORD_LAT)
                && dims[2].as_deref() == Some(COORD_LON)
        }
        None => false,
    }
}

/// Decompresses a 1-D coordinate chunk and returns its decoded little-endian `f64`
/// values (the `lat`/`lon` cell-center arrays).
///
/// Reads exactly the chunk at `<store>/<coord>/c/0` — a 1-D coordinate read
/// (architecture §1), never a `c/0/0/0` data chunk. The fixture stores the chunk
/// `bytes` (little-endian) + `zstd`-framed, so it is zstd-decoded with the pure-Rust
/// `ruzstd` decoder, then read as 8-byte little-endian doubles.
///
/// # Errors
///
/// | Condition | Error |
/// |---|---|
/// | the `<coord>/c/0` chunk is absent (the coordinate array is unread/unreadable) | [`CoreError::MissingGriddedCoordinate`] (the structurally required coordinate is missing) |
/// | the chunk cannot be zstd-decoded, or its length is not a multiple of 8 | [`CoreError::ZarrRead`] |
fn read_coord_f64(store: &Path, artifact: &str, coord: &str) -> Result<Vec<f64>, CoreError> {
    let chunk = store.join(coord).join("c").join("0");
    let raw = std::fs::read(&chunk).map_err(|_| CoreError::MissingGriddedCoordinate {
        artifact: artifact.to_string(),
        coordinate: coord.to_string(),
    })?;
    debug!(
        coordinate = coord,
        bytes = raw.len(),
        "read 1-D coordinate chunk (c/0, never c/0/0/0)"
    );

    let mut decoder =
        StreamingDecoder::new(std::io::Cursor::new(raw)).map_err(|e| CoreError::ZarrRead {
            artifact: artifact.to_string(),
            detail: format!("coordinate {coord:?} chunk is not a valid zstd frame: {e}"),
        })?;
    let mut decoded: Vec<u8> = Vec::new();
    decoder
        .read_to_end(&mut decoded)
        .map_err(|e| CoreError::ZarrRead {
            artifact: artifact.to_string(),
            detail: format!("coordinate {coord:?} chunk failed to decompress: {e}"),
        })?;

    if !decoded.len().is_multiple_of(8) {
        return Err(CoreError::ZarrRead {
            artifact: artifact.to_string(),
            detail: format!(
                "coordinate {coord:?} decoded to {} bytes, not a multiple of 8 (f64)",
                decoded.len()
            ),
        });
    }
    Ok(decoded
        .chunks_exact(8)
        .map(|c| {
            let mut buf = [0u8; 8];
            buf.copy_from_slice(c);
            f64::from_le_bytes(buf)
        })
        .collect())
}

/// Decompresses a 1-D coordinate chunk and returns its decoded little-endian `i64`
/// values (the `time` int64 day-count array).
///
/// Reads exactly the chunk at `<store>/<coord>/c/0` — a 1-D coordinate read
/// (architecture §1), never a `c/0/0/0` data chunk. The fixture stores the chunk
/// `bytes` (little-endian) + `zstd`-framed, so it is zstd-decoded with the same
/// pure-Rust `ruzstd` decoder [`read_coord_f64`] uses, then read as **8-byte
/// little-endian `i64`** — the correct decode for the int64 `time` coordinate (units
/// `days since 1970-01-01`), which the f64 leg would misread. No second store opener
/// and no `zarrs`/`ndarray` dependency: only the shared raw-chunk infra.
///
/// The returned values are the **raw day-counts** stored on disk; the day→microsecond
/// normalization is the separate [`normalize_days_to_micros`] helper, so callers (and
/// the round-trip test) can assert the decoded days before any scaling.
///
/// # Errors
///
/// | Condition | Error |
/// |---|---|
/// | the `<coord>/c/0` chunk is absent (the coordinate array is unread/unreadable) | [`CoreError::MissingGriddedCoordinate`] (the structurally required coordinate is missing) |
/// | the chunk cannot be zstd-decoded, or its length is not a multiple of 8 | [`CoreError::ZarrRead`] |
pub(crate) fn read_coord_i64(
    store: &Path,
    artifact: &str,
    coord: &str,
) -> Result<Vec<i64>, CoreError> {
    let chunk = store.join(coord).join("c").join("0");
    let raw = std::fs::read(&chunk).map_err(|_| CoreError::MissingGriddedCoordinate {
        artifact: artifact.to_string(),
        coordinate: coord.to_string(),
    })?;
    debug!(
        coordinate = coord,
        bytes = raw.len(),
        "read 1-D coordinate chunk as int64 (c/0, never c/0/0/0)"
    );

    let mut decoder =
        StreamingDecoder::new(std::io::Cursor::new(raw)).map_err(|e| CoreError::ZarrRead {
            artifact: artifact.to_string(),
            detail: format!("coordinate {coord:?} chunk is not a valid zstd frame: {e}"),
        })?;
    let mut decoded: Vec<u8> = Vec::new();
    decoder
        .read_to_end(&mut decoded)
        .map_err(|e| CoreError::ZarrRead {
            artifact: artifact.to_string(),
            detail: format!("coordinate {coord:?} chunk failed to decompress: {e}"),
        })?;

    if !decoded.len().is_multiple_of(8) {
        return Err(CoreError::ZarrRead {
            artifact: artifact.to_string(),
            detail: format!(
                "coordinate {coord:?} decoded to {} bytes, not a multiple of 8 (i64)",
                decoded.len()
            ),
        });
    }
    Ok(decoded
        .chunks_exact(8)
        .map(|c| {
            let mut buf = [0u8; 8];
            buf.copy_from_slice(c);
            i64::from_le_bytes(buf)
        })
        .collect())
}

/// Normalizes int64 `time` **day-counts** to i64 **microseconds** since the unix
/// epoch (the `* `[`MICROS_PER_DAY`] scale).
///
/// The gridded·dynamic Zarr `time` coordinate is integral days; the scalar/reduced
/// axis HDX compares against is i64 microseconds. Because the day-counts are integral
/// the conversion is exact (no float). The multiply is saturating to mirror the
/// producer's own `saturating_mul(MICROS_PER_DAY)` so the two axes are bit-identical;
/// the conformant fixture's day magnitudes are tiny, so saturation never triggers.
pub(crate) fn normalize_days_to_micros(days: &[i64]) -> Vec<i64> {
    days.iter()
        .map(|&d| d.saturating_mul(MICROS_PER_DAY))
        .collect()
}

/// Parses the root `zarr.json` and recovers the §8 inline consolidated-metadata map.
///
/// Reads the file once (the single consolidated read) and returns the
/// `consolidated_metadata.metadata` object plus the [`ConsolidatedMetadataSource`]
/// recording that the consolidated path was live. If the store carries no inline
/// consolidated metadata, returns an [`ConsolidatedMetadataSource::R3Skip`] reason
/// instead of the map (the caller then has no metadata and errors) — never a silent
/// claim.
///
/// # Errors
///
/// | Condition | Error |
/// |---|---|
/// | the root `zarr.json` is absent or cannot be read | [`CoreError::ZarrRead`] |
/// | the root `zarr.json` is not valid JSON | [`CoreError::ZarrRead`] |
fn read_consolidated_map(
    store: &Path,
    artifact: &str,
) -> Result<(serde_json::Map<String, Value>, ConsolidatedMetadataSource), CoreError> {
    let root = store.join("zarr.json");
    let bytes = std::fs::read(&root).map_err(|e| CoreError::ZarrRead {
        artifact: artifact.to_string(),
        detail: format!("root zarr.json unreadable: {e}"),
    })?;
    let json: Value = serde_json::from_slice(&bytes).map_err(|e| CoreError::ZarrRead {
        artifact: artifact.to_string(),
        detail: format!("root zarr.json is not valid JSON: {e}"),
    })?;

    // The §8 consolidated path: `consolidated_metadata.kind == "inline"` and a
    // `metadata` map of every member. Anything else is an R3 skip-with-reason.
    let consolidated = json.get("consolidated_metadata");
    let kind = consolidated
        .and_then(|c| c.get("kind"))
        .and_then(Value::as_str);
    let metadata = consolidated
        .and_then(|c| c.get("metadata"))
        .and_then(Value::as_object);

    match (kind, metadata) {
        (Some("inline"), Some(map)) => {
            let members: Vec<String> = map.keys().cloned().collect();
            info!(
                members = members.len(),
                "learned store via §8 inline consolidated metadata (one read)"
            );
            Ok((
                map.clone(),
                ConsolidatedMetadataSource::Consolidated { members },
            ))
        }
        _ => {
            let reason = match kind {
                Some(other) => format!(
                    "consolidated_metadata.kind is {other:?}, not \"inline\" (no inline map to read)"
                ),
                None => "root zarr.json has no consolidated_metadata object (uncosolidated store)"
                    .to_string(),
            };
            warn!(reason = %reason, "consolidated-metadata path unavailable (skip)");
            // No usable map: surface the skip reason via a typed error so the caller
            // never proceeds with a silently-claimed path.
            Err(CoreError::ZarrRead {
                artifact: artifact.to_string(),
                detail: reason,
            })
        }
    }
}

/// Resolves the CRS of a `grid_mapping` target array as a comparable `EPSG:<code>`
/// string when an EPSG id resolves, else the raw CRS string verbatim.
///
/// Follows the CF convention: the target array carries `spatial_ref` (often already
/// `"EPSG:<code>"`) and/or `crs_wkt`. The reader prefers `spatial_ref` when it is an
/// `EPSG:<code>` form; otherwise it records whatever string is present verbatim
/// (the raw-string fallback — documented, never silently claimed). HDX records the
/// CRS and compares nothing here (M5 is enforced elsewhere).
fn resolve_crs(target: &ArrayMetadataV3) -> Option<Crs> {
    if let Some(spatial_ref) = string_attr(target, "spatial_ref") {
        return Some(Crs::new(spatial_ref));
    }
    if let Some(wkt) = string_attr(target, "crs_wkt") {
        return Some(Crs::new(wkt));
    }
    None
}

/// Reads a `gridded_dynamic/<label>.zarr` store's metadata into a [`ZarrGrid`]
/// (spec §7/§8, architecture §1/§3.5).
///
/// Learns the store from **one read** of the root `zarr.json`'s §8 inline
/// consolidated-metadata map, classifies its arrays (coordinate vs data variable vs
/// `grid_mapping` target), reads the 1-D `lat`/`lon` coordinate chunks to derive the
/// center→edge [`GridExtent`], resolves the CRS from the data variables'
/// `grid_mapping` target, and maps each data variable to an ordinary `GriddedDynamic`
/// [`Field`]. The `time` coordinate's int64 day-counts are decoded (via
/// [`read_coord_i64`]) and normalized to i64 micros ([`gridded_time_micros`](ZarrGrid::gridded_time_micros))
/// — the comparable axis `validate` enforces; they are not used for the grid geometry.
/// **No `c/0/0/0` data chunk is ever read.**
///
/// `grid_label` is the label of the store (e.g. `era5`), supplied by the caller from
/// the artifact name; HDX names nothing from the file contents.
///
/// # Errors
///
/// | Condition | Error |
/// |---|---|
/// | the root `zarr.json` is absent / unreadable / not valid JSON, or carries no inline consolidated metadata | [`CoreError::ZarrRead`] |
/// | a member's metadata cannot be parsed as a Zarr v3 array | [`CoreError::ZarrRead`] |
/// | a required `lat` / `lon` / `time` coordinate array (or its `c/0` chunk) is absent | [`CoreError::MissingGriddedCoordinate`] |
/// | no data variable carries a resolvable `grid_mapping` target with a CRS | [`CoreError::MissingGridGeoref`] |
/// | a data variable's `data_type` does not map to a supported [`Dtype`] | [`CoreError::UnknownDtype`] |
#[instrument(skip(grid_label), fields(path = %path.as_ref().display()))]
pub fn read_zarr_grid(
    path: impl AsRef<Path>,
    grid_label: GridLabel,
) -> Result<ZarrGrid, CoreError> {
    let store = path.as_ref();
    let artifact = store.display().to_string();

    // One read of the root zarr.json via the §8 consolidated-metadata path.
    let (map, consolidated_source) = read_consolidated_map(store, &artifact)?;

    // Parse every member's metadata (typed via the R1 zarrs_metadata crate).
    let mut arrays: Vec<(String, ArrayMetadataV3)> = Vec::with_capacity(map.len());
    for (name, value) in &map {
        let meta: ArrayMetadataV3 =
            serde_json::from_value(value.clone()).map_err(|e| CoreError::ZarrRead {
                artifact: artifact.clone(),
                detail: format!("member {name:?} is not valid Zarr v3 array metadata: {e}"),
            })?;
        arrays.push((name.clone(), meta));
    }

    // Confirm the three coordinate arrays self-reference their dimensions (spec §7.3).
    // `time`/`lat`/`lon` are coordinate iff name + 1-D dimension self-reference.
    for coord in [COORD_TIME, COORD_LAT, COORD_LON] {
        let present = arrays
            .iter()
            .any(|(name, meta)| name == coord && self_references_dimension(meta, coord));
        if !present {
            return Err(CoreError::MissingGriddedCoordinate {
                artifact: artifact.clone(),
                coordinate: coord.to_string(),
            });
        }
    }

    // Data variables: arrays carrying a `grid_mapping` attribute and 3-D
    // `[time, lat, lon]` dims. The `grid_mapping` target (the `crs` array) is
    // resolved ONLY by following a data var's `grid_mapping` attribute — never by
    // dimension self-reference.
    let mut fields: Vec<Field> = Vec::new();
    let mut grid_mapping_target: Option<String> = None;

    for (name, meta) in &arrays {
        let Some(target) = string_attr(meta, GRID_MAPPING_ATTR) else {
            continue;
        };
        if !is_three_dim_grid(meta) {
            // A `grid_mapping` without 3-D [time, lat, lon] dims is not a data var.
            continue;
        }
        // First data var seen pins the grid_mapping target name.
        if grid_mapping_target.is_none() {
            grid_mapping_target = Some(target);
        }

        let dtype = zarr_dtype(meta.data_type.name())?;
        let units = Units::new(string_attr(meta, UNITS_ATTR));
        let standard_name = string_attr(meta, STANDARD_NAME_ATTR);
        let field = Field::new(
            FieldName::new(name.as_str()),
            Quadrant::GriddedDynamic,
            dtype,
            units,
            standard_name,
            Some(grid_label.clone()),
        )?;
        fields.push(field);
    }

    // Resolve the CRS by following the data var's grid_mapping target (G3). The
    // target array (`crs`, shape `[]`) is read ONLY now that a data var points at it.
    let crs = match grid_mapping_target.as_deref() {
        Some(target_name) => {
            let target = arrays
                .iter()
                .find(|(name, _)| name == target_name)
                .map(|(_, meta)| meta);
            match target.and_then(resolve_crs) {
                Some(crs) => crs,
                None => {
                    return Err(CoreError::MissingGridGeoref {
                        artifact: artifact.clone(),
                        detail: format!(
                            "grid_mapping target {target_name:?} has no spatial_ref/crs_wkt"
                        ),
                    });
                }
            }
        }
        None => {
            return Err(CoreError::MissingGridGeoref {
                artifact: artifact.clone(),
                detail: "no data variable declares a grid_mapping target".to_string(),
            });
        }
    };

    // 1-D coordinate reads (c/0 only): cell centers for the center→edge extent.
    let lon = read_coord_f64(store, &artifact, COORD_LON)?;
    let lat = read_coord_f64(store, &artifact, COORD_LAT)?;
    // The `time` coordinate is the int64 day-count array (NOT f64): decode it via the
    // int64 leg and normalize to i64 micros — the comparable axis `check_t2`/`check_m6`
    // (S5/S6) enforce. A 1-D c/0 read; never a c/0/0/0 data chunk.
    let gridded_time_micros =
        normalize_days_to_micros(&read_coord_i64(store, &artifact, COORD_TIME)?);
    if lon.len() < 2 || lat.len() < 2 {
        return Err(CoreError::ZarrRead {
            artifact: artifact.clone(),
            detail: "lat/lon coordinate arrays need at least two cells to derive a resolution"
                .to_string(),
        });
    }

    let width = lon.len();
    let height = lat.len();
    // Signed per-axis resolution from the first two centers (CF cell spacing).
    let x_res = lon[1] - lon[0];
    let y_res = lat[1] - lat[0];
    let resolution = GridResolution::new(x_res, y_res);

    // S1 center→edge: shift the first center out by half the *signed* resolution.
    let west = center_to_edge(lon[0], x_res);
    let north = center_to_edge(lat[0], y_res);
    let extent = GridExtent::from_edge_origin(west, north, x_res.abs(), width, height);

    let grid_info = GridInfo::new(grid_label, extent, resolution, width, height, crs);

    info!(
        fields = fields.len(),
        width,
        height,
        west = extent.west(),
        north = extent.north(),
        "read Zarr grid metadata (consolidated path, center→edge extent)"
    );

    Ok(ZarrGrid {
        grid_info,
        fields,
        consolidated_source,
        gridded_time_micros,
    })
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use crate::error::CoreError;
    use crate::field::{Dtype, Quadrant};
    use crate::newtypes::{Crs, GridLabel};
    use crate::zarr_reader::{
        ConsolidatedMetadataSource, MICROS_PER_DAY, normalize_days_to_micros, read_coord_i64,
        read_zarr_grid,
    };

    /// Resolves a path under the committed `conformance/` fixture tree.
    fn conformance(rel: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../conformance")
            .join(rel)
    }

    /// The basin-0001 fixture store path.
    fn fixture_store() -> PathBuf {
        conformance("valid/minimal/basin=0001/gridded_dynamic/era5.zarr")
    }

    /// Recursively copies a directory tree into a fresh temp dir, returning its path.
    fn copy_store_to_temp(src: &Path, tag: &str) -> PathBuf {
        let dst = std::env::temp_dir().join(format!(
            "hdx-zarr-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        copy_dir_all(src, &dst);
        dst
    }

    /// Copies `src` directory recursively into `dst` (test helper).
    fn copy_dir_all(src: &Path, dst: &Path) {
        std::fs::create_dir_all(dst).expect("create temp dir");
        for entry in std::fs::read_dir(src).expect("read_dir") {
            let entry = entry.expect("dir entry");
            let from = entry.path();
            let to = dst.join(entry.file_name());
            if from.is_dir() {
                copy_dir_all(&from, &to);
            } else {
                std::fs::copy(&from, &to).expect("copy file");
            }
        }
    }

    // --- Raw centers + converted edges (the half-pixel fix made visible) --------

    #[test]
    fn med5_consolidated_path_is_live_with_six_members() {
        let grid = read_zarr_grid(fixture_store(), GridLabel::new("era5"))
            .expect("fixture store must read via the consolidated path");

        match grid.consolidated_source() {
            ConsolidatedMetadataSource::Consolidated { members } => {
                // All six members enumerated from the single inline read.
                assert_eq!(members.len(), 6, "all six members from one read");
                for expected in [
                    "crs",
                    "era5_precipitation",
                    "era5_precipitation_was_filled",
                    "lat",
                    "lon",
                    "time",
                ] {
                    assert!(
                        members.iter().any(|m| m == expected),
                        "member {expected:?} must be enumerated"
                    );
                }
            }
            ConsolidatedMetadataSource::R3Skip { reason } => {
                panic!("expected the live consolidated path, got R3 skip: {reason}")
            }
        }
    }

    #[test]
    fn converted_edge_extent_matches_cog_at_10_50() {
        // The raw centers are lon[0]=10.125 / lat[0]=49.875 / res 0.25, width 6,
        // height 8 (separately pinned below). After S1's center→edge conversion the
        // extent edges are 10.0 / 50.0 / 11.5 / 48.0 — byte-matching the COG bounds.
        let grid =
            read_zarr_grid(fixture_store(), GridLabel::new("era5")).expect("fixture must read");
        let info = grid.grid_info();
        let extent = info.extent();

        assert_eq!(extent.west(), 10.0, "west edge");
        assert_eq!(extent.north(), 50.0, "north edge");
        assert_eq!(extent.east(), 11.5, "east edge");
        assert_eq!(extent.south(), 48.0, "south edge");

        // Raw centers / resolution / dims pinned to make the half-pixel step visible.
        assert_eq!(info.width(), 6, "lon has 6 cells");
        assert_eq!(info.height(), 8, "lat has 8 cells");
        assert_eq!(
            info.resolution().x_res(),
            0.25,
            "x_res +0.25 (east-marching)"
        );
        assert_eq!(
            info.resolution().y_res(),
            -0.25,
            "y_res -0.25 (south-marching)"
        );
    }

    #[test]
    fn raw_centers_and_resolution_are_pinned() {
        // Pin the raw center values the conversion starts from: west = lon[0]−res/2
        // = 10.125 − 0.125 = 10.0; north = lat[0] − y_res/2 = 49.875 + 0.125 = 50.0.
        // We assert via the public extent + the documented half-pixel formula so the
        // first centers (10.125 / 49.875) are the only values that reproduce them.
        let grid =
            read_zarr_grid(fixture_store(), GridLabel::new("era5")).expect("fixture must read");
        let info = grid.grid_info();
        let x_res = info.resolution().x_res();
        let y_res = info.resolution().y_res();

        // Reconstruct the first centers from the recorded edges + signed resolution.
        let lon0 = info.extent().west() + x_res / 2.0;
        let lat0 = info.extent().north() + y_res / 2.0;
        assert_eq!(lon0, 10.125, "lon[0] center");
        assert_eq!(lat0, 49.875, "lat[0] center");
        assert_eq!(x_res, 0.25);
        assert_eq!(y_res, -0.25);
    }

    // --- G1 self-naming + CF units + G3 CRS -------------------------------------

    #[test]
    fn g1_two_data_vars_are_ordinary_gridded_dynamic_fields() {
        let grid =
            read_zarr_grid(fixture_store(), GridLabel::new("era5")).expect("fixture must read");
        let fields = grid.fields();

        assert_eq!(fields.len(), 2, "exactly two data variables");
        let names: Vec<&str> = fields.iter().map(|f| f.name().as_str()).collect();
        assert!(names.contains(&"era5_precipitation"), "precip catalogued");
        assert!(
            names.contains(&"era5_precipitation_was_filled"),
            "the companion-mask name is an ordinary field — no name magic"
        );

        for field in fields {
            assert_eq!(
                field.quadrant(),
                Quadrant::GriddedDynamic,
                "no channel axis; ordinary gridded·dynamic quadrant"
            );
            assert_eq!(
                field.grid_label(),
                Some(&GridLabel::new("era5")),
                "grid_label == era5 for both"
            );
        }
    }

    #[test]
    fn cf_units_and_dtype_for_precipitation() {
        let grid =
            read_zarr_grid(fixture_store(), GridLabel::new("era5")).expect("fixture must read");
        let precip = grid
            .fields()
            .iter()
            .find(|f| f.name().as_str() == "era5_precipitation")
            .expect("precip field present");
        assert_eq!(precip.units().as_deref(), Some("mm"), "CF units == mm");
        assert_eq!(precip.dtype(), Dtype::F32, "float32 → f32");
    }

    #[test]
    fn g3_crs_resolved_via_grid_mapping_target_is_epsg_4326() {
        // The `crs` array (shape [], no dimension_names) is reached ONLY by following
        // a data var's grid_mapping attr — not by dimensions. Its spatial_ref records
        // EPSG:4326.
        let grid =
            read_zarr_grid(fixture_store(), GridLabel::new("era5")).expect("fixture must read");
        assert_eq!(grid.grid_info().crs(), &Crs::new("EPSG:4326"));
    }

    // --- No-data-chunk gate -----------------------------------------------------

    #[test]
    fn low3_no_data_chunk_read_identical_metadata_after_deleting_c000() {
        // Read once over the pristine fixture for the baseline metadata + extent.
        let baseline =
            read_zarr_grid(fixture_store(), GridLabel::new("era5")).expect("baseline must read");

        // Copy the store to temp and DELETE the era5_precipitation data chunk
        // (c/0/0/0). Coordinate chunks (lat/lon/time c/0) are kept.
        let temp = copy_store_to_temp(&fixture_store(), "nochunk");
        let data_chunk = temp
            .join("era5_precipitation")
            .join("c")
            .join("0")
            .join("0")
            .join("0");
        assert!(data_chunk.exists(), "fixture copy must have the data chunk");
        std::fs::remove_file(&data_chunk).expect("delete data chunk");
        // Also delete the companion mask's data chunk.
        let mask_chunk = temp
            .join("era5_precipitation_was_filled")
            .join("c")
            .join("0")
            .join("0")
            .join("0");
        if mask_chunk.exists() {
            std::fs::remove_file(&mask_chunk).expect("delete mask data chunk");
        }

        let after = read_zarr_grid(&temp, GridLabel::new("era5"));
        std::fs::remove_dir_all(&temp).ok();
        let after = after.expect("read must STILL succeed with no data chunk");

        // Identical metadata + extent: proves no c/0/0/0 data chunk was read.
        assert_eq!(
            after.grid_info(),
            baseline.grid_info(),
            "grid metadata identical without the data chunk"
        );
        assert_eq!(
            after.fields(),
            baseline.fields(),
            "field catalog identical without the data chunk"
        );
    }

    // --- Negative paths ---------------------------------------------------------

    #[test]
    fn missing_lon_coordinate_returns_missing_gridded_coordinate() {
        // Copy the store and remove the `lon` array's metadata from the consolidated
        // map so `lon` is no longer a self-referencing coordinate array.
        let temp = copy_store_to_temp(&fixture_store(), "nolon");
        let root = temp.join("zarr.json");
        let text = std::fs::read_to_string(&root).expect("read root");
        let mut json: serde_json::Value = serde_json::from_str(&text).expect("parse root");
        json["consolidated_metadata"]["metadata"]
            .as_object_mut()
            .expect("metadata map")
            .remove("lon");
        std::fs::write(&root, serde_json::to_string(&json).expect("serialize")).expect("write");

        let result = read_zarr_grid(&temp, GridLabel::new("era5"));
        std::fs::remove_dir_all(&temp).ok();

        match result {
            Err(CoreError::MissingGriddedCoordinate {
                artifact,
                coordinate,
            }) => {
                assert!(!artifact.is_empty(), "the store path is named");
                assert_eq!(coordinate, "lon");
            }
            other => panic!("expected MissingGriddedCoordinate for lon, got {other:?}"),
        }
    }

    #[test]
    fn data_var_without_grid_mapping_target_returns_missing_grid_georef() {
        // Copy the store and strip the `grid_mapping` attribute from both data vars
        // so no data variable points at a grid_mapping target.
        let temp = copy_store_to_temp(&fixture_store(), "nogeoref");
        let root = temp.join("zarr.json");
        let text = std::fs::read_to_string(&root).expect("read root");
        let mut json: serde_json::Value = serde_json::from_str(&text).expect("parse root");
        for var in ["era5_precipitation", "era5_precipitation_was_filled"] {
            json["consolidated_metadata"]["metadata"][var]["attributes"]
                .as_object_mut()
                .expect("attributes map")
                .remove("grid_mapping");
        }
        std::fs::write(&root, serde_json::to_string(&json).expect("serialize")).expect("write");

        let result = read_zarr_grid(&temp, GridLabel::new("era5"));
        std::fs::remove_dir_all(&temp).ok();

        match result {
            Err(CoreError::MissingGridGeoref { artifact, detail }) => {
                assert!(!artifact.is_empty(), "the store path is named");
                assert!(!detail.is_empty());
            }
            other => panic!("expected MissingGridGeoref, got {other:?}"),
        }
    }

    #[test]
    fn missing_store_returns_zarr_read() {
        match read_zarr_grid("/no/such/store.zarr", GridLabel::new("era5")) {
            Err(CoreError::ZarrRead { artifact, detail }) => {
                assert!(artifact.contains("store.zarr"));
                assert!(!detail.is_empty());
            }
            other => panic!("expected ZarrRead, got {other:?}"),
        }
    }

    #[test]
    fn uncosolidated_store_records_skip_reason_not_silent() {
        // A root zarr.json without consolidated metadata is surfaced as a typed
        // ZarrRead with a stated reason (never a silent claim).
        let temp = copy_store_to_temp(&fixture_store(), "unconsolidated");
        let root = temp.join("zarr.json");
        let text = std::fs::read_to_string(&root).expect("read root");
        let mut json: serde_json::Value = serde_json::from_str(&text).expect("parse root");
        json.as_object_mut()
            .expect("root object")
            .remove("consolidated_metadata");
        std::fs::write(&root, serde_json::to_string(&json).expect("serialize")).expect("write");

        let result = read_zarr_grid(&temp, GridLabel::new("era5"));
        std::fs::remove_dir_all(&temp).ok();

        match result {
            Err(CoreError::ZarrRead { detail, .. }) => {
                assert!(
                    detail.contains("consolidated_metadata"),
                    "the skip reason names the missing consolidated metadata: {detail}"
                );
            }
            other => panic!("expected ZarrRead with a skip reason, got {other:?}"),
        }
    }

    // --- PRE-LAND GATE i: int64 /time chunk round-trip ---------------------------

    #[test]
    fn read_coord_i64_round_trips_int64_days_over_fixture_time_chunk() {
        // The /time array is int64, shape [5], units "days since 1970-01-01" (confirmed
        // in zarr.json). read_coord_f64 (the only pre-existing decoder) reads 8-byte LE
        // *doubles* — WRONG for int64 /time. read_coord_i64 reads the SAME c/0 chunk
        // (34 bytes, zstd-framed) through the SAME ruzstd StreamingDecoder infra, decoded
        // as i64 LE. Assert the exact int64 day-counts the fixture stored.
        let store = fixture_store();
        let artifact = store.display().to_string();
        let days = read_coord_i64(&store, &artifact, "time")
            .expect("read_coord_i64 must decode the int64 /time chunk");

        // Bit-exact vs orthographos's read_zarr_time_axis day-counts (gridded.rs:664
        // decodes the same array as ArrayD<i64>): 2000-01-01..2000-01-05.
        assert_eq!(
            days,
            vec![10957_i64, 10958, 10959, 10960, 10961],
            "the int64 day-counts the fixture /time chunk stored, bit-exactly"
        );

        // normalize_days_to_micros applies the *86_400_000_000 scale (orthographos
        // axis.rs:24 / exec.rs); integral days -> exact i64 micros (no float).
        let micros = normalize_days_to_micros(&days);
        assert_eq!(
            MICROS_PER_DAY, 86_400_000_000_i64,
            "one day in microseconds"
        );
        assert_eq!(
            micros,
            vec![
                10957_i64 * MICROS_PER_DAY,
                10958 * MICROS_PER_DAY,
                10959 * MICROS_PER_DAY,
                10960 * MICROS_PER_DAY,
                10961 * MICROS_PER_DAY,
            ],
            "days -> i64 micros is the exact *86_400_000_000 scale"
        );
    }

    #[test]
    fn read_coord_i64_missing_chunk_returns_missing_gridded_coordinate() {
        // A coord whose c/0 chunk is absent surfaces the structural-coordinate error
        // (same MissingGriddedCoordinate leg read_coord_f64 uses).
        match read_coord_i64(&fixture_store(), "art", "no_such_coord") {
            Err(CoreError::MissingGriddedCoordinate {
                artifact,
                coordinate,
            }) => {
                assert_eq!(artifact, "art");
                assert_eq!(coordinate, "no_such_coord");
            }
            other => panic!("expected MissingGriddedCoordinate, got {other:?}"),
        }
    }
}
