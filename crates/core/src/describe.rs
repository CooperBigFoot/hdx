//! The `describe` self-description type and its serializable wire shape (spec §10, R4).
//!
//! This module stands up [`Description`] — the full, **facts-only** self-description
//! `describe` emits — and the describe-local `#[derive(Serialize)]` DTO layer that
//! defines its JSON shape **in one place** (architecture §3.5/§5, R4). The verb itself
//! (the IO that reads `manifest.json` + runs `discover`) is a later step; this module is
//! **pure**: the only mapping it owns is `Discovery + Manifest → Description → DTO`.
//!
//! ## The R4 mini-contract (why a describe-local DTO)
//!
//! The `Description` JSON shape is a downstream contract (the CLI and the PyO3 binding
//! consume it), so it is owned by the `describe` boundary, **not** by the inert domain
//! types. The domain types ([`Field`], [`GridInfo`], [`Manifest`],
//! [`TimeExtent`](crate::scalar_reader::TimeExtent)) gain **no** `serde::Serialize`
//! derive; instead a private DTO layer (`DescriptionDto` and friends) mirrors the
//! discovered facts and carries the `#[derive(Serialize)]`. This is the same two-stage
//! discipline the manifest parser uses with its private `ManifestDto`: the wire shape is
//! a single, reviewable surface that cannot silently drift with internal type changes.
//! The shape is versioned **implicitly by `format_version` only** (the hard cut, spec
//! §0/§11) — there is no separate schema-version field.
//!
//! ## Facts only — no verdict (spec §10)
//!
//! `describe` **reports facts, never a conformance verdict**. There is no `conformant`
//! key, no §14 check outcome, anywhere in this shape (that is `validate`, a later
//! milestone). A discovery gap is reported as a **fact**: a basin with no
//! `scalar_dynamic.parquet` has its time-extent entry serialized with a `null` extent
//! (the §6.1 ragged fact), and an absent `outlines.geoparquet` yields an empty
//! `delineations` array — never a raised error or a verdict.
//!
//! ## The floor stress-test, made reviewable (spec §10/§11)
//!
//! Every datum in [`Description`] comes from **exactly one** source — either one of the
//! six manifest floor fields **or** a named [`Discovery`] accessor. The per-DTO-field
//! source annotations below make this auditable in one place. If assembling a needed
//! fact ever required something that is *neither* a manifest field *nor* a discovery
//! accessor, the correct response is to **flag a spec/floor bug and amend the
//! architecture — never add a manifest field** (spec §11).
//!
//! The DTO shape is, by construction:
//!
//! - top level: exactly `{manifest, basins, fields, grids, time_extents, delineations}`;
//! - `manifest`: exactly the six floor fields
//!   `{format_version, name, created_at, producer_version, crs, cadence}`;
//! - a `fields` entry: exactly `{name, quadrant, dtype, units, grid_label}` — every
//!   field (including the companion-mask `era5_precipitation_was_filled` and the
//!   `{source}_{variable}` `era5_precipitation`) carries this **same** key set, with no
//!   `mask` / `belongs_to` / `source` / `variable` magic (spec §2).
//!
//! ## Glossary
//!
//! | Term | Meaning |
//! |---|---|
//! | [`Description`] | the full self-description `describe` emits — manifest + discovered facts, no verdict |
//! | [`BasinTimeExtent`] | a per-basin ragged time-extent entry: a [`BasinId`] paired with `Option<TimeExtent>` (`None` = a recorded gap, spec §6.1) |
//! | DTO layer | the describe-local `#[derive(Serialize)]` types that own the JSON wire shape (R4); the domain types stay free of `serde::Serialize` |
//! | R4 mini-contract | the `Description` JSON shape as a downstream contract, versioned implicitly by `format_version` only |

use serde::Serialize;
use serde::ser::Error as _;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use tracing::{debug, instrument};

use crate::field::Field;
use crate::grid::GridInfo;
use crate::gridded_discovery::Discovery;
use crate::manifest::Manifest;
use crate::newtypes::BasinId;
use crate::scalar_reader::{TimeExtent, TimeExtentSource};

/// A per-basin ragged time-extent entry (spec §6.1) — the §6.1 ragged fact.
///
/// Pairs a [`BasinId`] with its `Option<TimeExtent>`: `Some(..)` when the basin's
/// `scalar_dynamic.parquet` yielded a `[start, end]` span, and `None` when it did not
/// (a recorded **gap**, never a verdict). Basins may legitimately span different periods
/// of record (spec §6.1), so the entries are surfaced verbatim, in basin order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BasinTimeExtent {
    basin_id: BasinId,
    time_extent: Option<TimeExtent>,
}

impl BasinTimeExtent {
    /// Borrows the basin this entry belongs to.
    pub fn basin_id(&self) -> &BasinId {
        &self.basin_id
    }

    /// Returns the basin's time extent, or `None` for a recorded gap (spec §6.1).
    pub fn time_extent(&self) -> Option<TimeExtent> {
        self.time_extent
    }
}

/// The full self-description `describe` emits (spec §10, architecture §3.5).
///
/// Composed **only** from the six manifest floor fields + discovered facts — the
/// inert/agnostic floor stress test (spec §10/§11). Fields are private; read them via
/// the getters. Build one with [`Description::from_discovery`] (pure, no IO) and
/// serialize it through [`Description::to_dto`] / [`Description::to_json_string`].
///
/// It records **facts only, never a verdict** (spec §10): discovery gaps are reported
/// as `None` extents / empty lists. It is **inert/agnostic** (spec §1): every member is
/// a structural fact, and it adds no manifest-floor or derivable field.
#[derive(Debug, Clone, PartialEq)]
pub struct Description {
    manifest: Manifest,
    basins: Vec<BasinId>,
    fields: Vec<Field>,
    grids: Vec<GridInfo>,
    time_extents: Vec<BasinTimeExtent>,
    delineations: Vec<crate::newtypes::DelineationLabel>,
}

impl Description {
    /// Assembles a [`Description`] from a parsed [`Manifest`] and a [`Discovery`] — the
    /// pure mapping, **no IO** (spec §10, architecture §3.5/§5).
    ///
    /// Reads through the documented public accessors only; it does **not** reshape
    /// [`Discovery`] (the MS3/MS4 contract). Each member is sourced from exactly one
    /// place (the floor stress test, spec §10/§11):
    ///
    /// | `Description` member | Single source |
    /// |---|---|
    /// | `manifest` | the parsed [`Manifest`] (the six floor fields) |
    /// | `basins` | [`Discovery::basins`] |
    /// | `fields` | [`Discovery::fields`] (`scalar ⊕ gridded`, concatenated, no merge) |
    /// | `grids` | [`Discovery::grids`] |
    /// | `time_extents` | [`Discovery::basins`] zipped with the scalar half's per-basin [`time_extent`](crate::discovery::BasinScalar::time_extent) |
    /// | `delineations` | [`Discovery::delineations`] |
    ///
    /// The per-basin time extents are read from the scalar half's `per_basin` facts in
    /// basin order, so a basin with no extent records `None` (the §6.1 ragged gap).
    #[instrument(skip(manifest, discovery))]
    pub fn from_discovery(manifest: &Manifest, discovery: &Discovery) -> Self {
        let basins: Vec<BasinId> = discovery.basins().to_vec();
        let fields: Vec<Field> = discovery.fields().into_iter().cloned().collect();
        let grids: Vec<GridInfo> = discovery.grids().to_vec();
        let delineations: Vec<crate::newtypes::DelineationLabel> =
            discovery.delineations().to_vec();

        // The per-basin ragged extents (spec §6.1): one entry per scalar-half basin,
        // in basin order, pairing the folder id with its `Option<TimeExtent>`.
        let time_extents: Vec<BasinTimeExtent> = discovery
            .scalar()
            .per_basin()
            .iter()
            .map(|basin| BasinTimeExtent {
                basin_id: basin.basin_id_folder().clone(),
                time_extent: basin.time_extent(),
            })
            .collect();

        debug!(
            basins = basins.len(),
            fields = fields.len(),
            grids = grids.len(),
            time_extents = time_extents.len(),
            delineations = delineations.len(),
            "assembled Description from the discovery layer"
        );

        Self {
            manifest: manifest.clone(),
            basins,
            fields,
            grids,
            time_extents,
            delineations,
        }
    }

    /// Borrows the parsed manifest (the six floor fields).
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// Borrows the discovered basin list, in stable sorted order.
    pub fn basins(&self) -> &[BasinId] {
        &self.basins
    }

    /// Borrows the unified field catalog (`scalar ⊕ gridded`, spec §5).
    pub fn fields(&self) -> &[Field] {
        &self.fields
    }

    /// Borrows the per-grid representative geometries (spec §7).
    pub fn grids(&self) -> &[GridInfo] {
        &self.grids
    }

    /// Borrows the per-basin ragged time-extent entries (spec §6.1).
    pub fn time_extents(&self) -> &[BasinTimeExtent] {
        &self.time_extents
    }

    /// Borrows the distinct delineation labels (spec §9).
    pub fn delineations(&self) -> &[crate::newtypes::DelineationLabel] {
        &self.delineations
    }

    /// Maps this [`Description`] into its serializable [`DescriptionDto`] (R4 shape).
    ///
    /// The DTO owns the JSON wire shape; this is the single place the domain types are
    /// projected onto it. Borrowing — no clones beyond what the DTO references need.
    pub fn to_dto(&self) -> DescriptionDto<'_> {
        DescriptionDto {
            manifest: ManifestDto::from_manifest(&self.manifest),
            basins: self.basins.iter().map(BasinId::as_str).collect(),
            fields: self.fields.iter().map(FieldDto::from_field).collect(),
            grids: self.grids.iter().map(GridDto::from_grid_info).collect(),
            time_extents: self
                .time_extents
                .iter()
                .map(BasinTimeExtentDto::from_entry)
                .collect(),
            delineations: self
                .delineations
                .iter()
                .map(crate::newtypes::DelineationLabel::as_str)
                .collect(),
        }
    }

    /// Serializes this [`Description`] to a compact JSON string (the R4 wire shape).
    ///
    /// # Errors
    ///
    /// | Condition | Error |
    /// |---|---|
    /// | the DTO cannot be serialized (e.g. `created_at` cannot be RFC 3339 formatted) | [`serde_json::Error`] |
    pub fn to_json_string(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(&self.to_dto())
    }

    /// Serializes this [`Description`] to a pretty-printed JSON string (the R4 wire
    /// shape, indented; the form a golden snapshot pins).
    ///
    /// # Errors
    ///
    /// | Condition | Error |
    /// |---|---|
    /// | the DTO cannot be serialized (e.g. `created_at` cannot be RFC 3339 formatted) | [`serde_json::Error`] |
    pub fn to_json_pretty(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(&self.to_dto())
    }
}

/// The serializable top-level `describe` shape (R4). Owns the JSON wire shape.
///
/// Exactly `{manifest, basins, fields, grids, time_extents, delineations}` — no
/// `conformant` / verdict key (facts only, spec §10). Each field's source is the
/// `Description` member of the same name.
#[derive(Debug, Serialize)]
pub struct DescriptionDto<'a> {
    /// Source: the six floor fields of the parsed [`Manifest`].
    manifest: ManifestDto<'a>,
    /// Source: [`Description::basins`].
    basins: Vec<&'a str>,
    /// Source: [`Description::fields`] (`scalar ⊕ gridded`).
    fields: Vec<FieldDto<'a>>,
    /// Source: [`Description::grids`].
    grids: Vec<GridDto<'a>>,
    /// Source: [`Description::time_extents`] (the §6.1 ragged facts).
    time_extents: Vec<BasinTimeExtentDto<'a>>,
    /// Source: [`Description::delineations`].
    delineations: Vec<&'a str>,
}

/// The serializable manifest shape — **exactly** the six floor fields (spec §11).
///
/// Describe-local: it does **not** touch the manifest parser's own raw `ManifestDto`.
/// `created_at` is emitted as the RFC 3339 string (so the wire value matches the
/// manifest input exactly), the rest as their borrowed producer strings.
#[derive(Debug, Serialize)]
struct ManifestDto<'a> {
    /// Source: `Manifest::format_version` (the hard cut; always `"0.1"`).
    format_version: &'a str,
    /// Source: `Manifest::name`.
    name: &'a str,
    /// Source: `Manifest::created_at`, formatted as a strict RFC 3339 string.
    #[serde(serialize_with = "serialize_rfc3339")]
    created_at: OffsetDateTime,
    /// Source: `Manifest::producer_version`.
    producer_version: &'a str,
    /// Source: `Manifest::crs`.
    crs: &'a str,
    /// Source: `Manifest::cadence`.
    cadence: &'a str,
}

impl<'a> ManifestDto<'a> {
    /// Projects a parsed [`Manifest`] onto the wire shape (the six floor fields only).
    fn from_manifest(manifest: &'a Manifest) -> Self {
        Self {
            format_version: manifest.format_version().as_str(),
            name: manifest.name().as_str(),
            created_at: manifest.created_at(),
            producer_version: manifest.producer_version().as_str(),
            crs: manifest.crs().as_str(),
            cadence: manifest.cadence().as_str(),
        }
    }
}

/// The serializable field shape — **exactly** `{name, quadrant, dtype, units,
/// grid_label}` (spec §2).
///
/// Every field carries this same key set, regardless of name. `units` and `grid_label`
/// are `string | null` (absent → JSON `null`). The quadrant is the stable explicit 2×2
/// (`temporal` + `shape`), so a consumer reads both axes without re-deriving them.
#[derive(Debug, Serialize)]
struct FieldDto<'a> {
    /// Source: `Field::name` (the verbatim producer string).
    name: &'a str,
    /// Source: `Field::quadrant`, as the explicit 2×2 `{temporal, shape}`.
    quadrant: QuadrantDto,
    /// Source: `Field::dtype`, via its canonical `as_str()`.
    dtype: &'a str,
    /// Source: `Field::units` (`string | null` — never invented).
    units: Option<&'a str>,
    /// Source: `Field::grid_label` (`string | null`; present iff the field is gridded).
    grid_label: Option<&'a str>,
}

impl<'a> FieldDto<'a> {
    /// Projects an ordinary [`Field`] onto the wire shape (no name-pattern magic).
    fn from_field(field: &'a Field) -> Self {
        Self {
            name: field.name().as_str(),
            quadrant: QuadrantDto::from_quadrant(field.quadrant()),
            dtype: field.dtype().as_str(),
            units: field.units().as_deref(),
            grid_label: field.grid_label().map(crate::newtypes::GridLabel::as_str),
        }
    }
}

/// The serializable quadrant shape — the explicit 2×2 `{temporal, shape}` (spec §2).
///
/// HDX classifies a field on two independent axes; emitting both verbatim keeps the
/// wire shape self-documenting (a consumer never re-derives the axes from a packed
/// string). The axis values are the stable lowercase pole names.
#[derive(Debug, Serialize)]
struct QuadrantDto {
    /// `"static"` or `"dynamic"` — the temporal axis (source: `Quadrant::temporal`).
    temporal: &'static str,
    /// `"scalar"` or `"gridded"` — the shape axis (source: `Quadrant::shape`).
    shape: &'static str,
}

impl QuadrantDto {
    /// Splits a [`Quadrant`](crate::field::Quadrant) into its two stable axis strings.
    fn from_quadrant(quadrant: crate::field::Quadrant) -> Self {
        let temporal = match quadrant.temporal() {
            crate::field::Temporal::Static => "static",
            crate::field::Temporal::Dynamic => "dynamic",
        };
        let shape = match quadrant.shape() {
            crate::field::Shape::Scalar => "scalar",
            crate::field::Shape::Gridded => "gridded",
        };
        Self { temporal, shape }
    }
}

/// The serializable per-grid geometry shape (spec §7).
///
/// One entry per discovered grid artifact: its label, the cell-edge extent, the signed
/// per-axis resolution, the pixel dimensions, and the recorded CRS string.
#[derive(Debug, Serialize)]
struct GridDto<'a> {
    /// Source: `GridInfo::grid_label`.
    grid_label: &'a str,
    /// Source: `GridInfo::extent` (the NW cell-edge origin + far edges).
    extent: GridExtentDto,
    /// Source: `GridInfo::resolution` (signed per axis).
    resolution: GridResolutionDto,
    /// Source: `GridInfo::width` (the x / column count).
    width: usize,
    /// Source: `GridInfo::height` (the y / row count).
    height: usize,
    /// Source: `GridInfo::crs` (the recorded CRS string).
    crs: &'a str,
}

impl<'a> GridDto<'a> {
    /// Projects a [`GridInfo`] onto the wire shape.
    fn from_grid_info(grid: &'a GridInfo) -> Self {
        let extent = grid.extent();
        let resolution = grid.resolution();
        Self {
            grid_label: grid.grid_label().as_str(),
            extent: GridExtentDto {
                west: extent.west(),
                north: extent.north(),
                east: extent.east(),
                south: extent.south(),
            },
            resolution: GridResolutionDto {
                x_res: resolution.x_res(),
                y_res: resolution.y_res(),
            },
            width: grid.width(),
            height: grid.height(),
            crs: grid.crs().as_str(),
        }
    }
}

/// The serializable grid-extent shape — the four cell-edge coordinates (spec §7).
#[derive(Debug, Serialize)]
struct GridExtentDto {
    /// The west cell-edge coordinate (the NW origin's x).
    west: f64,
    /// The north cell-edge coordinate (the NW origin's y).
    north: f64,
    /// The east cell-edge coordinate (the far x).
    east: f64,
    /// The south cell-edge coordinate (the far y).
    south: f64,
}

/// The serializable grid-resolution shape — the signed per-axis steps (spec §7).
#[derive(Debug, Serialize)]
struct GridResolutionDto {
    /// The signed x-axis (east-west) resolution.
    x_res: f64,
    /// The signed y-axis (north-south) resolution.
    y_res: f64,
}

/// The serializable per-basin time-extent entry (spec §6.1).
///
/// A basin id paired with its extent, `null` when the basin has no recorded extent (the
/// §6.1 ragged gap). The extent itself carries `{start, end, source}` — `start`/`end`
/// as RFC 3339 strings, `source` as the read-tier provenance string.
#[derive(Debug, Serialize)]
struct BasinTimeExtentDto<'a> {
    /// Source: `BasinTimeExtent::basin_id`.
    basin_id: &'a str,
    /// Source: `BasinTimeExtent::time_extent` (`null` = a recorded gap).
    time_extent: Option<TimeExtentDto>,
}

impl<'a> BasinTimeExtentDto<'a> {
    /// Projects a [`BasinTimeExtent`] onto the wire shape.
    fn from_entry(entry: &'a BasinTimeExtent) -> Self {
        Self {
            basin_id: entry.basin_id().as_str(),
            time_extent: entry.time_extent().map(TimeExtentDto::from_extent),
        }
    }
}

/// The serializable time-extent shape — `{start, end, source}` (spec §6.1/§8).
#[derive(Debug, Serialize)]
struct TimeExtentDto {
    /// Source: `TimeExtent::start`, as a strict RFC 3339 string.
    #[serde(serialize_with = "serialize_rfc3339")]
    start: OffsetDateTime,
    /// Source: `TimeExtent::end`, as a strict RFC 3339 string.
    #[serde(serialize_with = "serialize_rfc3339")]
    end: OffsetDateTime,
    /// Source: `TimeExtent::source` (which read tier produced the extent).
    source: &'static str,
}

impl TimeExtentDto {
    /// Projects a [`TimeExtent`] onto the wire shape (timestamps as RFC 3339 strings).
    fn from_extent(extent: TimeExtent) -> Self {
        let source = match extent.source() {
            TimeExtentSource::Statistics => "statistics",
            TimeExtentSource::BoundedColumnScan => "bounded_column_scan",
        };
        Self {
            start: extent.start().as_offset_date_time(),
            end: extent.end().as_offset_date_time(),
            source,
        }
    }
}

/// Serializes an [`OffsetDateTime`] as a strict RFC 3339 string.
///
/// Used for `created_at` and the time-extent boundaries so every wire timestamp matches
/// the strict RFC 3339 form the manifest parser accepts. A formatting failure (which a
/// validly-constructed datetime cannot trigger) is surfaced as a serde error — never a
/// panic (library code carries no `unwrap`/`expect`).
fn serialize_rfc3339<S>(value: &OffsetDateTime, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    let formatted = value.format(&Rfc3339).map_err(S::Error::custom)?;
    serializer.serialize_str(&formatted)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};

    use serde_json::Value;

    use crate::describe::Description;
    use crate::gridded_discovery::{Discovery, discover};
    use crate::manifest::Manifest;

    /// Resolves a path under the committed `conformance/` fixture tree.
    ///
    /// `CARGO_MANIFEST_DIR` is `crates/core`; the fixtures live two levels up.
    fn conformance(rel: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../conformance")
            .join(rel)
    }

    /// The exact six-field manifest of the MS2 valid fixture (decoded facts).
    const VALID_MANIFEST: &str = r#"{
  "format_version": "0.1",
  "name": "hdx-conformance-valid-minimal",
  "created_at": "2026-06-01T00:00:00Z",
  "producer_version": "hdx-fixtures 0.1.0",
  "crs": "EPSG:4326",
  "cadence": "daily"
}"#;

    /// Builds the `(Manifest, Discovery)` pair for the valid fixture (the inputs the
    /// pure assembler consumes — no `describe` verb yet).
    fn valid_inputs() -> (Manifest, Discovery) {
        let manifest = Manifest::from_json(VALID_MANIFEST).expect("the valid manifest must parse");
        let discovery = discover(conformance("valid/minimal")).expect("the valid fixture discovers");
        (manifest, discovery)
    }

    /// Returns the set of top-level keys of a JSON object value.
    fn object_keys(value: &Value) -> BTreeSet<String> {
        value
            .as_object()
            .expect("expected a JSON object")
            .keys()
            .cloned()
            .collect()
    }

    #[test]
    fn from_discovery_maps_every_fact_one_to_one() {
        let (manifest, discovery) = valid_inputs();
        let description = Description::from_discovery(&manifest, &discovery);

        // Basins: exactly the three, in stable sorted order (Discovery::basins).
        let basins: Vec<&str> = description.basins().iter().map(|b| b.as_str()).collect();
        assert_eq!(basins, vec!["0001", "0002", "0003"]);

        // Unified field order = scalar ⊕ gridded, concatenated (Discovery::fields).
        let names: Vec<&str> = description
            .fields()
            .iter()
            .map(|f| f.name().as_str())
            .collect();
        assert_eq!(
            names,
            vec![
                "drainage_area",
                "streamflow",
                "elevation",
                "era5_precipitation",
                "era5_precipitation_was_filled"
            ],
            "fields = scalar ⊕ gridded, in order, no merge"
        );

        // Grids: mapped 1:1 from Discovery::grids (COG + Zarr for the shared era5).
        assert_eq!(
            description.grids().len(),
            discovery.grids().len(),
            "grids mapped 1:1"
        );
        for grid in description.grids() {
            assert_eq!(grid.crs().as_str(), "EPSG:4326");
        }

        // Ragged time extents: one entry per basin, in basin order (spec §6.1), all
        // present on the conformant fixture and all from Statistics.
        let extent_basins: Vec<&str> = description
            .time_extents()
            .iter()
            .map(|e| e.basin_id().as_str())
            .collect();
        assert_eq!(extent_basins, vec!["0001", "0002", "0003"]);
        for entry in description.time_extents() {
            let extent = entry
                .time_extent()
                .expect("each fixture basin has an extent");
            assert_eq!(
                extent.source(),
                crate::scalar_reader::TimeExtentSource::Statistics
            );
        }

        // Delineations: mapped 1:1 from Discovery::delineations.
        let mut delineations: Vec<&str> = description
            .delineations()
            .iter()
            .map(|d| d.as_str())
            .collect();
        delineations.sort_unstable();
        assert_eq!(delineations, vec!["grit", "merit"]);

        // Manifest carried through unchanged (the six floor fields).
        assert_eq!(
            description.manifest().name().as_str(),
            "hdx-conformance-valid-minimal"
        );
    }

    #[test]
    fn dto_top_level_key_set_is_exactly_the_six_facts_keys_no_verdict() {
        let (manifest, discovery) = valid_inputs();
        let description = Description::from_discovery(&manifest, &discovery);

        let value: Value =
            serde_json::to_value(description.to_dto()).expect("the DTO must serialize");

        // Exactly {manifest, basins, fields, grids, time_extents, delineations}.
        let keys = object_keys(&value);
        let expected: BTreeSet<String> = [
            "manifest",
            "basins",
            "fields",
            "grids",
            "time_extents",
            "delineations",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(keys, expected, "exact top-level key set (facts only)");

        // No verdict key anywhere at the top level (facts only — spec §10).
        assert!(
            !value.as_object().expect("object").contains_key("conformant"),
            "describe emits no `conformant` verdict key"
        );
    }

    #[test]
    fn manifest_sub_object_is_exactly_the_six_floor_keys() {
        let (manifest, discovery) = valid_inputs();
        let description = Description::from_discovery(&manifest, &discovery);
        let value: Value = serde_json::to_value(description.to_dto()).expect("serialize");

        let manifest_value = value.get("manifest").expect("manifest object present");
        let keys = object_keys(manifest_value);
        let expected: BTreeSet<String> = [
            "format_version",
            "name",
            "created_at",
            "producer_version",
            "crs",
            "cadence",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(keys, expected, "manifest = exactly the six floor keys");

        // created_at is emitted as the RFC 3339 string verbatim (matches the input).
        assert_eq!(
            manifest_value.get("created_at").and_then(Value::as_str),
            Some("2026-06-01T00:00:00Z")
        );
    }

    #[test]
    fn every_field_sub_object_has_exactly_the_ordinary_key_set() {
        let (manifest, discovery) = valid_inputs();
        let description = Description::from_discovery(&manifest, &discovery);
        let value: Value = serde_json::to_value(description.to_dto()).expect("serialize");

        let fields = value
            .get("fields")
            .and_then(Value::as_array)
            .expect("fields array present");

        let expected: BTreeSet<String> = ["name", "quadrant", "dtype", "units", "grid_label"]
            .iter()
            .map(|s| s.to_string())
            .collect();

        for field in fields {
            assert_eq!(
                object_keys(field),
                expected,
                "field = exactly {{name, quadrant, dtype, units, grid_label}}"
            );
        }
    }

    #[test]
    fn companion_mask_and_source_variable_fields_are_ordinary() {
        // The companion-mask `era5_precipitation_was_filled` and the
        // `{source}_{variable}` `era5_precipitation` serialize with the SAME field key
        // set as every other field — no mask / belongs_to / source / variable keys
        // (spec §2 ordinariness).
        let (manifest, discovery) = valid_inputs();
        let description = Description::from_discovery(&manifest, &discovery);
        let value: Value = serde_json::to_value(description.to_dto()).expect("serialize");

        let fields = value
            .get("fields")
            .and_then(Value::as_array)
            .expect("fields array");

        let ordinary_keys: BTreeSet<String> =
            ["name", "quadrant", "dtype", "units", "grid_label"]
                .iter()
                .map(|s| s.to_string())
                .collect();

        for target in ["era5_precipitation", "era5_precipitation_was_filled"] {
            let entry = fields
                .iter()
                .find(|f| f.get("name").and_then(Value::as_str) == Some(target))
                .unwrap_or_else(|| panic!("{target} present in the catalog"));
            assert_eq!(
                object_keys(entry),
                ordinary_keys,
                "{target} carries the ordinary field key set, no name magic"
            );
            // It is a gridded·dynamic field on the era5 grid — verbatim, no split.
            let quadrant = entry.get("quadrant").expect("quadrant object");
            assert_eq!(
                quadrant.get("temporal").and_then(Value::as_str),
                Some("dynamic")
            );
            assert_eq!(
                quadrant.get("shape").and_then(Value::as_str),
                Some("gridded")
            );
            assert_eq!(
                entry.get("grid_label").and_then(Value::as_str),
                Some("era5")
            );
        }
    }

    #[test]
    fn time_extent_entry_shape_is_basin_id_plus_nullable_start_end_source() {
        let (manifest, discovery) = valid_inputs();
        let description = Description::from_discovery(&manifest, &discovery);
        let value: Value = serde_json::to_value(description.to_dto()).expect("serialize");

        let extents = value
            .get("time_extents")
            .and_then(Value::as_array)
            .expect("time_extents array");
        assert_eq!(extents.len(), 3, "one entry per basin (ragged §6.1)");

        let entry_keys: BTreeSet<String> = ["basin_id", "time_extent"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let inner_keys: BTreeSet<String> = ["start", "end", "source"]
            .iter()
            .map(|s| s.to_string())
            .collect();

        for entry in extents {
            assert_eq!(object_keys(entry), entry_keys);
            let inner = entry.get("time_extent").expect("time_extent present");
            assert_eq!(object_keys(inner), inner_keys);
            assert_eq!(
                inner.get("source").and_then(Value::as_str),
                Some("statistics")
            );
        }
    }
}
