# `hdx-core`

## Purpose

`hdx-core` holds **all contract logic for HDX v0.1** (Hydrology Dataset Exchange) —
the spec and its validator are the same artifact, so the two contract-executing verbs
`validate` and `describe` (spec §10) live here. This is the crate's agent entry-point:
start here to orient before reading `src/`. As of milestone MS1, the crate establishes
the **parse-don't-validate type model** — opaque domain newtypes, the crate-wide
`CoreError`, the `FormatVersion` hard cut, the field 2×2 quadrant model with a closed
`Dtype`, and the six-field `Manifest` boundary parser. Raw input (JSON strings,
producer-chosen strings) is converted into **valid-by-construction** domain types at
the boundary; every type downstream of that boundary is conformant by construction.

As of milestone MS3, the crate adds the **scalar half of the shared discovery layer**
both verbs will stand on (architecture §3.5/§5): the basin-first hive walk
([`layout`](src/layout.rs)), the scalar-parquet **metadata** reader
([`scalar_reader`](src/scalar_reader.rs)), and the single boundary function
([`discovery`](src/discovery.rs)) that ties them into one typed in-memory model. This
layer **reads metadata and 1-D coordinate columns only — never gridded chunks**
(architecture §1) and **records facts, never a verdict** (the §14 conformance checks
are MS6).

As of milestone MS4, the crate **completes the discovery layer** by adding its
**gridded / geometry half** and the **combined model** both verbs consume: the shared
grid value types ([`grid`](src/grid.rs)) with the single cell-edge convention, the
Zarr v3 metadata reader ([`zarr_reader`](src/zarr_reader.rs)), the pure-Rust COG /
GeoTIFF tag reader ([`cog_reader`](src/cog_reader.rs)), the `outlines.geoparquet`
reader ([`geoparquet_reader`](src/geoparquet_reader.rs)), and the assembler
([`gridded_discovery`](src/gridded_discovery.rs)) that ties them into a
`GriddedDiscovery` and **pairs** it with MS3's `ScalarDiscovery` in one `Discovery`
model **without reshaping either half**. The gridded readers read **metadata + 1-D
coordinate arrays + georef only — never a gridded chunk or pixel raster** (architecture
§1, LOW-3), and the assembler **observes** the spec §8 *shared grid label ⇒ alignment*
G2 precondition (the COG and Zarr extents now coincide at `10.0`/`50.0`) but **records
facts, never a verdict** (G2 enforcement is MS6). The `validate` / `describe` verbs
themselves land in later milestones, built on these types.

## Inert and agnostic (load-bearing discipline)

> **HDX describes the *shape* of data, never *what was done to it*** (spec §1).

No type or field in this crate carries — or may ever be extended to carry — a
**transform / normalization** state, a **role** (target / forcing / future-known), a
**semantic type** (continuous / categorical), a **gridded → lumped reduction**, or any
**provenance of computation**. A prediction dataset is just an HDX dataset. Two
concrete consequences enforced by the types here:

- The [`Manifest`](src/manifest.rs) is **exactly six fields** (the §11 floor) — no
  seventh, derivable field (no content hash, no data version, no field catalog, no
  basin list). Everything else is *discovered* from the files, never *declared*.
- [`FormatVersion`](src/format_version.rs) is a **hard cut**: a single-arm enum whose
  parse accepts only `"0.1"`. No multi-version reader is representable.

The MS3 discovery layer holds the same line. The scalar reader catalogs fields **purely
by physical schema** — a column named `streamflow_was_filled` is an *ordinary* field
with no suffix magic, no role, no belongs-to (spec §2). The discovery model adds **no**
manifest-floor field and **no** derivable / role / transform / semantic / provenance
field: the only provenance it records is *which read tier produced an extent*
(`Statistics` vs `BoundedColumnScan`), which is a property of the **read**, not of the
data — never a transform of the values.

Every time a design drifts toward encoding *what was done* or *what the data is for*,
it is wrong.

## Architecture — module map

Each node is a file under `crates/core/src/`. An edge `A --> B` means *module `A`
depends on module `B`*.

```mermaid
graph TD
    newtypes[newtypes — opaque producer-string wrappers]
    error[error — CoreError thiserror enum]
    format_version[format_version — FormatVersion hard cut]
    field[field — Temporal/Shape/Quadrant/Dtype/Units/Field]
    grid[grid — GridResolution/GridExtent/GridInfo + edge convention]
    manifest[manifest — six-field Manifest boundary parse]
    parquet_meta[parquet_meta — private: parquet footer/metadata only]
    layout[layout — basin-first hive walk into LayoutModel]
    scalar_reader[scalar_reader — scalar-parquet metadata reader]
    discovery[discovery — scalar half of the shared discovery layer]
    zarr_reader[zarr_reader — Zarr v3 metadata reader]
    cog_reader[cog_reader — COG/GeoTIFF tag reader]
    geoparquet_reader[geoparquet_reader — outlines schema + delineation/basin_id + CRS]
    gridded_discovery[gridded_discovery — gridded half + combined Discovery]

    format_version --> error
    field --> error
    field --> newtypes
    grid --> newtypes
    manifest --> error
    manifest --> newtypes
    manifest --> format_version

    parquet_meta --> error
    layout --> error
    layout --> newtypes
    scalar_reader --> error
    scalar_reader --> newtypes
    scalar_reader --> field
    scalar_reader --> parquet_meta
    discovery --> error
    discovery --> newtypes
    discovery --> field
    discovery --> layout
    discovery --> scalar_reader

    zarr_reader --> error
    zarr_reader --> newtypes
    zarr_reader --> field
    zarr_reader --> grid
    cog_reader --> error
    cog_reader --> newtypes
    cog_reader --> field
    cog_reader --> grid
    geoparquet_reader --> error
    geoparquet_reader --> newtypes
    geoparquet_reader --> parquet_meta
    gridded_discovery --> error
    gridded_discovery --> newtypes
    gridded_discovery --> field
    gridded_discovery --> grid
    gridded_discovery --> layout
    gridded_discovery --> discovery
    gridded_discovery --> zarr_reader
    gridded_discovery --> cog_reader
    gridded_discovery --> geoparquet_reader
```

The three MS3 modules (`layout` → `scalar_reader` → `discovery`) form the **scalar half
of the discovery layer**; `discovery` is its single boundary function. The four MS4
modules (`grid`, then the three readers `zarr_reader` / `cog_reader` /
`geoparquet_reader`, then `gridded_discovery`) form the **gridded / geometry half**;
`gridded_discovery` is its single boundary function (`discover_gridded`) and also the
home of the **combined** `Discovery` model (`discover`) that pairs both halves without
reshaping either. The `parquet_meta` node is private (the crate's one parquet
touchpoint, R1) — the helper both the scalar reader and the geoparquet reader are
layered on.

- **`newtypes`** — the leaf: opaque `String` wrappers (`BasinId`, `FieldName`,
  `GridLabel`, `DelineationLabel`, `Crs`, `Cadence`, `DatasetName`, `ProducerVersion`)
  with private fields, so confusion-prone values cannot be swapped at a call site. It
  depends on nothing else in the crate.
- **`error`** — the crate-wide [`CoreError`](src/error.rs); every fallible path
  returns it. Variants use named fields and are doc-commented with *when* they fire;
  several are intentional skeletons reserved for later milestones.
- **`format_version`** — the one contract-version axis, encoded as the hard cut.
- **`field`** — the field model: the two axes as enums, the four quadrants, the closed
  `Dtype` with a fallible boundary parse, opaque `Units`, and `Field` (whose
  constructor enforces `grid_label.is_some()` ⇔ `Shape::Gridded`).
- **`manifest`** — the system boundary: turns a raw `manifest.json` string into a
  valid-by-construction `Manifest` (it depends on `format_version` and `newtypes`).
- **`layout`** (MS3) — the basin-first hive walk: turns a dataset directory into a
  typed [`LayoutModel`](src/layout.rs) — the two **root rollups**' presence facts and
  the enumerated `basin=<id>` directories (folder id parsed out, per-basin artifact
  paths recorded). **Structure-only** (no bytes read inside any artifact); hidden /
  OS-cruft entries are ignored (pre-empts an MS6 L3 false positive). The single
  canonical doc for the cruft filter and the records-facts-not-verdicts stance lives in
  this module's `//!`.
- **`scalar_reader`** (MS3) — the scalar-parquet **metadata** reader: the arrow schema
  mapped to MS1 [`Field`](src/field.rs)s, the `basin_id` column (presence + in-file
  value), the [`time` descriptor](src/scalar_reader.rs), and the **per-basin time
  extent** with its provenance. The single canonical doc for the **LOW-1 bounded
  fallback** and the **MED-5 hand-off** lives in this module's `//!`.
- **`discovery`** (MS3) — the **scalar half** of the shared discovery layer: the typed
  [`ScalarDiscovery`](src/discovery.rs) model and the one boundary function
  [`discover_scalar`](src/discovery.rs) both verbs consume. The single canonical doc for
  the **MS4 seam** (where the gridded / geometry half attaches) lives in this module's
  `//!`.
- **`grid`** (MS4) — the shared gridded-geometry value types: `GridResolution` (signed
  per-axis), `GridExtent` (the **single north-west cell-EDGE origin convention** plus
  the Zarr center→edge half-pixel rule), and `GridInfo` (per-grid-label representative
  geometry + recorded `Crs`). Pure types, **no IO and no new crates**; the load-bearing
  cell-edge convention (the prior structural-misread defect's fix) is stated here and
  is what makes the G2 precondition observable. No type holds a pixel buffer (LOW-3).
- **`zarr_reader`** (MS4) — the Zarr v3 **metadata** reader for
  `gridded_dynamic/<label>.zarr` stores: learns the store from **one read** of the
  root `zarr.json`'s §8 inline **consolidated metadata** (MED-5, recorded as a
  `ConsolidatedMetadataSource` enum), classifies its arrays, reads the 1-D
  `lat`/`lon`/`time` coordinate chunks (`c/0`, never a `c/0/0/0` data chunk — LOW-3),
  and builds a `GridInfo` via the center→edge conversion plus one ordinary
  `GriddedDynamic` `Field` per data variable.
- **`cog_reader`** (MS4) — the COG / GeoTIFF **metadata** reader for
  `gridded_static/<label>.tif` artifacts, **tags only**: the band description
  (= field name) + units from tag 42112 `GDAL_METADATA` (the MED-4 protocol, resolved
  live; recorded as a `CogBandSource` enum) and the standard GeoTIFF georef tags into
  an edge-based `GridInfo` plus one ordinary `GriddedStatic` `Field`. Never decodes a
  pixel raster (LOW-3); its extent matches the Zarr reader's at `10.0`/`50.0`.
- **`geoparquet_reader`** (MS4) — the `outlines.geoparquet` **metadata + 1-D column**
  reader (reuses the private `parquet_meta` touchpoint, no new crate): the
  `basin_id`/`delineation`/`geometry` column-presence facts (Geo1), the distinct
  `delineation` labels (spec §9) and `basin_id` values (I1), the not-partitioned fact,
  and the `geo` KV PROJJSON CRS recorded as a comparable `EPSG:<code>` from its `id`
  (with a `CrsSource` enum flagging the raw-string + R3 fallback). The `geometry` blob
  is never decoded.
- **`gridded_discovery`** (MS4) — the **gridded / geometry half** of the shared
  discovery layer and the **combined** model: the typed
  [`GriddedDiscovery`](src/gridded_discovery.rs) (per-grid geometries, the gridded
  field catalog, the delineation labels, and the per-basin observed grid labels — the
  **G2 precondition fact** — plus the MED-5 Zarr path) assembled by
  [`discover_gridded`](src/gridded_discovery.rs), and the
  [`Discovery`](src/gridded_discovery.rs) struct that **pairs** it with MS3's
  `ScalarDiscovery` without reshaping either (the unified `basins` / `fields =
  scalar ⊕ gridded` / `grids` / `delineations` view). [`discover`](src/gridded_discovery.rs)
  builds both halves in one call. It **observes** the G2 precondition (the COG and
  Zarr extents coincide); it renders **no verdict** (enforcement is MS6).

The committed `schemas/manifest.schema.json` (at the repo root) mirrors the `manifest`
floor; a `jsonschema` dev-dependency test (`tests/manifest_schema.rs`) asserts the
schema and the parser agree, so neither can drift.

## Glossary

Domain terms an agent would not infer from the code alone. Spec section in parentheses.

| Term | Meaning |
|---|---|
| **field** (§2) | The unit of HDX. A scientific variable, a QC mask, a cluster id, and a model prediction are *all just fields*; HDX privileges none. A field carries only `name`, `quadrant`, `dtype`, `units`, and (iff gridded) a `grid_label`. |
| **Temporal** (§2) | The time axis of a field — an enum, never a bool: `Static` (one value) vs `Dynamic` (a time series). |
| **Shape** (§2) | The space axis of a field — an enum, never a bool: `Scalar` (a single value) vs `Gridded` (a per-cell field over the basin bbox). Deliberately *not* "lumped vs gridded": "lumped" smuggles in a reduction, whereas a scalar value (outlet streamflow) is often scalar by nature. |
| **quadrant** (§2) | The product `Temporal × Shape` — one of `ScalarStatic`, `ScalarDynamic`, `GriddedStatic`, `GriddedDynamic`. A **per-field** classification, never a dataset-level mode: one dataset's schema may freely mix all four. |
| **Dtype** (§1, §2) | The physical element encoding (`f32`, `f64`, `i32`, `i64`, `bool`, `timestamp`). A **closed** enum — HDX recognizes exactly these and *rejects* anything else (no `Other(String)`, no panic). It is **opaque to semantics**: HDX records *how a value is encoded*, never *what it means* (continuous/categorical is the consumer's job). |
| **Units** (§1, §2) | An **opaque, optional** producer string — recorded verbatim or absent, never parsed (no unit algebra, no canonicalization, no vocabulary). |
| **basin_id** (§3) | A basin's id, **unique within the dataset** — the only requirement. How it is minted (gauge id, hash, integer, UUID) is the producer's business. It is the authoritative in-file id; a later milestone (MS6) cross-checks it against the `basin=<id>` partition folder. |
| **grid label** (§8) | A stable, producer-chosen name for a *grid family*; the gridded artifact is named after it (the file stem — `era5.tif` / `era5.zarr` → `era5`). A label **shared across the `gridded_static` and `gridded_dynamic` subtrees signals cell-for-cell alignment** — an invariant checked across basins in MS6, not here. |
| **GridExtent cell-edge convention** (§7, arch §3.5) | HDX records a grid's extent as a **north-west cell-EDGE origin** (`west`/`north`) plus the far edges (`east`/`south`) — the GeoTIFF-native form, *not* a cell center. The COG reader takes the GeoTIFF tiepoint verbatim (already edge-based); the Zarr reader converts its cell-**center** coordinate arrays to edges with the **half-pixel rule** (`west = lon[0] − x_res/2`, `north = lat[0] − y_res/2`, signs per axis). This single convention is load-bearing: two genuinely-aligned artifacts yield *identical* extents (`10.0`/`50.0` on the fixture), so the G2 precondition is observable — the prior structural-misread defect's fix. |
| **GridInfo** (§7, arch §3.5) | The per-grid-label **representative geometry**: a `GridExtent`, a signed `GridResolution`, the pixel `width`/`height`, and the recorded `Crs`. HDX records it and interprets none of it (no canonical unit, no reprojection). One `GridInfo` per observed artifact; the COG and Zarr `GridInfo` for a shared label sit side by side so MS6 can compare them. |
| **consolidated metadata** (§8, MED-5) | The §8 inline map in a Zarr v3 store's root `zarr.json` (`consolidated_metadata.kind == "inline"`) that carries **every member's metadata in one object**, so the whole store is learned from a single read. The Zarr reader records which path it took (`Consolidated` vs an `R3Skip` reason) as an enum — never silently claimed; a writer/reader mismatch is fixed by **regenerating the fixture**, never a reader hack. |
| **shared grid label ⇒ alignment (G2 precondition)** (§8, §14 G2) | Spec §8: when the *same* grid label appears in both the `gridded_static` (COG) and `gridded_dynamic` (Zarr) subtrees, the two artifacts are **cell-for-cell aligned**. MS4 **observes** this — it records the shared label and that the two `GridInfo` extents coincide — but renders **no verdict**. MS6 *enforces* G2 over this recorded fact. |
| **delineation** (§9) | A **neutral** label on each outline polygon (MERIT, GRIT, HydroBASINS, a custom run, a hand-drawn polygon) — *not* assumed to name a published hydrofabric. HDX interprets nothing; disagreement between delineations is itself a modeling signal. Read from the `outlines.geoparquet` `delineation` column; the distinct labels (`{grit, merit}` on the fixture) feed the discovery model. |
| **cadence** (§6, §11) | The dataset-wide cadence / calendar convention (e.g. `"daily"`), a declared manifest convention. A non-empty opaque string here; cross-checked against the realized time axes in a later milestone. |
| **manifest floor** (§11) | The manifest is the *irreducible floor*: **exactly six fields** — `format_version`, `name`, `created_at`, `producer_version`, `crs`, `cadence` — and nothing else. Adding any derivable field is a conformance bug, so it is unrepresentable. |
| **`format_version` hard cut** (§0, §11) | `format_version` is read **first** and is a **hard cut**: only `"0.1"` is accepted (exact-string, no numeric coercion — `"0.10"` ≠ `"0.1"`); anything else is rejected outright before any other field is interpreted. |
| **discovery layer** (arch §3.5/§5) | The shared in-memory model both verbs stand on (`describe` *reports* it, `validate` *checks rules over it*). MS3 built the **scalar half** (`ScalarDiscovery`) — basin list, scalar field catalog, per-basin `time` descriptors + extents, `basin_id` observations, root-rollup presence. MS4 **completes** it with the **gridded / geometry half** (`GriddedDiscovery`) — per-grid `GridInfo`s, the gridded field catalog, the delineation labels, and the per-basin observed grid labels + MED-5 Zarr path — and pairs both halves in the combined `Discovery` model (`basins`, `fields = scalar ⊕ gridded`, `grids`, `delineations`) **without reshaping either**. The layer **records facts, never a verdict** (§14 enforcement is MS6). |
| **layout model** (§4) | The typed model of one dataset's on-disk **structure** ([`LayoutModel`](src/layout.rs)), produced by the basin-first hive walk: the root-rollup presence facts plus the enumerated `basin=<id>` dirs with per-basin artifact paths. **Structure-only** — no bytes are read inside any artifact (architecture §1). Hidden / OS-cruft entries (`.DS_Store`, `.gitkeep`, dotfiles) are ignored, so working-tree cruft is never enumerated as an HDX path (pre-empts an MS6 L3 stray-file false positive). |
| **root rollup** (§4) | A **dataset-level** artifact at the dataset root (`scalar_static.parquet`, `outlines.geoparquet`) — distinct from the per-basin artifacts under `basin=<id>/`. The asymmetry is principled (size/shape): small dataset-level data rolls up to the root, large per-basin data stays partitioned. MS3 records each rollup's **presence** as a fact (L1 enforcement is MS6); it parses the `scalar_static` schema but only records the `outlines` presence (the outlines schema is MS4). |
| **folder id vs in-file `basin_id`** (§3) | Two ids per basin, recorded **side by side**: the **folder id** parsed from the `basin=<id>` directory name is *locality*; the **in-file `basin_id`** column read from the parquet is *authority*. MS3 surfaces the pair; it does **not** decide agreement (MS6's I2 cross-check does). The in-file value is read via a bounded 1-D key-column read (architecture §1). |
| **time extent** (§6.1) | A basin's `[start, end]` timestamp pair on the `time` axis. Basins may legitimately span **different periods of record** (ragged extents, §6.1) — MS3 surfaces this as a fact, never raised. |
| **time-extent provenance** (§8, arch §7 R3) | Which path produced an extent, recorded on it as an enum (never a `bool`): `Statistics` (the `time` column's row-group min/max from the parquet footer — the primary, no-data-read path, §8) vs `BoundedColumnScan` (the LOW-1 fallback). Downstream (`describe` MS5, `validate` MS6 R3 classification) reports which tier produced each extent. |
| **LOW-1 bounded fallback** (§8, arch §1/§7 R3) | When the `time` column carries **no** row-group statistics, the reader recovers `[min, max]` by reading **exactly one column — `time`, selected by name** (never a data column, never a gridded chunk). It is classified under **R3** as a **metadata/index-tier, architecture-§1-compliant 1-D coordinate read** (the same tier as reading a Zarr `time` coordinate array), **NOT** a gridded-chunk value decode. The bound is hard and stated in [`scalar_reader`](src/scalar_reader.rs)'s `//!`. |
| **MED-5 hand-off** (§8) | The MS2→MS3 writer/reader hand-off: MS2 self-asserted (pyarrow side) that the valid fixture's `time` column carries usable row-group statistics; MS3 **confirms it Rust-side** (a test asserts the statistics are readable by the `parquet` crate **and** that the extent comes from `Statistics`, not the fallback). **The rule:** if Rust ever cannot surface the statistics pyarrow wrote, the fix is to **regenerate the MS2 fixture**, **never** a reader workaround — a mismatch is a generator bug. Stated canonically in [`scalar_reader`](src/scalar_reader.rs)'s `//!`. |
