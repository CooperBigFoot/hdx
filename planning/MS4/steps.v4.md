# MS4 — Gridded + geometry metadata readers (discovery layer, gridded/geometry half)

> **Milestone scope (verbatim intent, milestones.md MS4).** Complete the shared
> discovery layer: read **Zarr v3 array metadata + 1-D `time`/`lat`/`lon` coordinate
> arrays + CF `grid_mapping`/CRS**, read **COG band descriptions + standard georef
> tags**, and read **`outlines.geoparquet` schema + `delineation`/`basin_id` columns**.
> Land the hard **R1** crate decision for Zarr / COG / geometry, and explicitly
> resolve the **§8 consolidated-metadata / sharding** read question. **Metadata + 1-D
> coordinate reads only — NEVER gridded chunks** (architecture §1). The output feeds
> the unified discovery model alongside MS3's [`ScalarDiscovery`].
>
> **Hard boundaries (do not cross).** No `regrid` / `clip` / `reduce`. No
> inert-violating field anywhere (no transform / role / semantic / provenance). The
> manifest stays exactly the six floor fields. No `describe` / `validate` verb logic
> (MS5 / MS6) — MS4 only *reads facts*; it enforces no spec §14 check (it produces the
> on-disk preconditions the validator later checks). No gridded-chunk decode and no
> pixel-raster read anywhere in `hdx-core`.

---

## Ground truth (verified against the committed MS2 fixture before planning)

Every number below was decoded from `conformance/valid/minimal/` at plan time, so the
acceptance assertions are byte-true (this is the fix for the prior HIGH defect — the
plan now agrees with the bytes).

| Fact | Verified value | Source |
|---|---|---|
| Zarr `lat` (basin 0001) | `lat[0]=49.875 … lat[7]=48.125`, n=8, **cell centers** | decoded `lat/c/0` |
| Zarr `lon` (basin 0001) | `lon[0]=10.125 … lon[5]=11.375`, n=6, **cell centers**, res=0.25 | decoded `lon/c/0` |
| Zarr center→edge | `west = lon[0] − res/2 = 10.0`; `north = lat[0] + res/2 = 50.0` | arithmetic |
| COG affine tiepoint | `transform = (0.25, 0, 10.0, 0, −0.25, 50.0)` → **cell edges** west=10.0 north=50.0 | rasterio |
| COG bounds | left=10.0 bottom=48.0 right=11.5 top=50.0, 6×8 | rasterio |
| Both describe | the **same** 6×8 / 0.25° grid (Zarr center = COG edge + ½ pixel) | arithmetic |
| Zarr consolidated metadata | `consolidated_metadata.kind == "inline"` in root `zarr.json`, all **six** members present (`crs`, `era5_precipitation`, `era5_precipitation_was_filled`, `lat`, `lon`, `time`) | root `zarr.json` |
| Zarr `era5_precipitation` codec | `sharding_indexed` (v3 sharding); data chunk at `c/0/0/0`; coord chunks at `c/0` | `zarr.json` |
| Zarr `crs` array | `shape: []`, **no** `dimension_names`; resolved only via a data var's `grid_mapping: "crs"` attr | `zarr.json` |
| COG georef tags | 33550 ModelPixelScale, 33922 ModelTiepoint (cnt 6), 34735 GeoKeyDirectory, 34736/34737, tile tags 322–325 present | raw IFD |
| COG band description | stored in **tag 42112 GDAL_METADATA** XML: `<Item name="DESCRIPTION" sample="0" role="description">elevation</Item>` (NOT IFD tag 270 — no 270 present) | raw IFD |
| COG band units | same tag 42112 XML: `<Item name="units" sample="0">m</Item>` | raw IFD |
| geoparquet schema | columns `[basin_id, delineation, geometry]`, 4 rows | pyarrow |
| geoparquet `geo` KV | `primary_column == "geometry"`; CRS is a **PROJJSON object** `id == {authority:"EPSG", code:4326}` (NOT the string `"EPSG:4326"`) | pyarrow `geo` KV |
| geoparquet delineations | `{grit, merit}`; basin 0001 carries both (≥2 labels → §9 plurality) | pyarrow |
| manifest CRS | `"EPSG:4326"` | `manifest.json` |

**The single grid convention this milestone adopts (S1).** `GridExtent` records the
**north-west cell-EDGE origin** plus per-axis signed resolution (GeoTIFF-native). The
COG reader takes the affine tiepoint verbatim (already edge-based). The Zarr reader
converts its cell-CENTER coordinate arrays to edges with the half-pixel rule
`west = lon[0] − x_res/2`, `north = lat[0] + y_res/2` (signs per axis). On the fixture
both yield **west=10.0 / north=50.0**, so S5's "the two extents coincide" assertion is
byte-true for two genuinely-aligned artifacts. This convention is the load-bearing
fix; it is stated in the `GridInfo` doc and recorded as an architecture amendment.

**The single CRS-recording rule this milestone adopts (S1/S4).** A reader records
[`Crs`] as a comparable `EPSG:<code>` string whenever an EPSG authority/code is
resolvable (Zarr `spatial_ref`/`crs_wkt` → EPSG; COG GeoKeyDirectory → EPSG;
geoparquet PROJJSON `id.authority=="EPSG"` → `EPSG:<code>`), so MS6's M5 cross-check
compares like to like against the manifest's `"EPSG:4326"`. When no EPSG id is
resolvable, the reader records the raw CRS string verbatim and flags that file's M5
readiness as an **R3** item (documented, never silently claimed). This is *recording
discipline*, not the M5 *rule* (the rule is MS6).

---

## Ordering rationale

MS4 attaches the gridded + geometry half of the discovery layer onto MS3's proven
scalar half, **without reshaping** `ScalarDiscovery`. The steps are strictly
dependency-sequential and each leaves the tree green (build + test + clippy
`-D warnings`):

1. **S1 — Shared gridded/geometry types + the one grid convention + R1/MED-4 decision
   record.** Pure types and one architecture-amendment block, zero new IO crates. It
   defines `GridExtent` (the single cell-edge convention), `GridResolution`,
   `GridInfo`, the gridded-field/grid catalog shape, the delineation list, and the
   error variants the readers will raise — and records the R1 crate choices + the
   MED-4 three-outcome COG-band protocol + the §8 consolidated-metadata gate decision
   **before** any reader is written, so the readers are coded against a settled
   contract. Types-first mirrors the repo's parse-don't-validate discipline (MS1) and
   makes the half-pixel convention reviewable in one place. Green with unit tests over
   the convention math + a no-pixel-read review-gate doc.

2. **S2 — Zarr v3 reader (consolidated metadata + 1-D coords + CF georef).** Adds the
   Zarr crate, reads the store **via the §8 consolidated-metadata path** (one read of
   the root `zarr.json`), classifies arrays into gridded·dynamic fields vs coordinate
   arrays vs the `grid_mapping` target, reads the 1-D `lat`/`lon`/`time` coordinate
   arrays (never a `c/0/0/0` data chunk), and builds `GridInfo` with the center→edge
   conversion. This is the highest-risk reader (MED-5 consolidated path + the
   half-pixel fix), so it lands first among the readers and is locked with
   fixture-backed tests asserting the raw center values **and** the converted edges.

3. **S3 — COG reader (band descriptions + georef tags).** Adds the TIFF crate, reads
   band descriptions (= field names, the MED-4 crux), band units, and the standard
   georef tags into an edge-based `GridInfo`. Depends on S1's `GridInfo`/convention
   and is independent of S2; sequenced after S2 so the `GridInfo` shape is already
   exercised by a live reader. The MED-4 three-outcome protocol fires here with a
   fixture round-trip test.

4. **S4 — geoparquet reader (schema + delineation/basin_id + CRS).** Reuses MS3's
   pure-Rust `parquet` stack to read the geoparquet schema and the `delineation` /
   `basin_id` columns, plus the `geo` KV PROJJSON CRS recorded as `EPSG:<code>`.
   Independent of S2/S3; sequenced after them to keep the readers grouped and to land
   the CRS-recording rule once both gridded readers already use it.

5. **S5 — assemble the gridded/geometry half + the combined discovery model.** Ties
   S2+S3+S4 into one `GriddedDiscovery` (and a `Discovery` that pairs it with MS3's
   `ScalarDiscovery`), walking the per-basin gridded subtree paths already on
   `LayoutModel`. This is where the shared-grid-label-across-subtrees **G2
   precondition** is *observed* (read, not enforced) and where the end-to-end
   "the COG `GridInfo` and the Zarr `GridInfo` extents coincide at 10.0/50.0"
   assertion lives — now byte-true because of S1's single convention. Last because it
   composes the four prior steps and completes the discovery layer both verbs consume.

Each step is one conventional commit, each runs `./scripts/bump-version.sh patch` +
stages `Cargo.toml` + tags `v<version>`.

---

## Scope-guard statement

No step implements `regrid` / `clip` / `reduce`, a reduction, or any hydrology
operation. No type or field introduced carries transform, role, semantic type, or
provenance — every datum is a structural fact (a name, a quadrant, a dtype, optional
units, a grid label, an extent/resolution, a CRS string, a delineation label, a
presence flag). The manifest stays exactly the six floor fields (MS4 adds nothing to
`Manifest`). No step performs a spec §14 *verdict*: MS4 reads facts and records the
G1 / G2-precondition / G3 / Geo1 / I1 on-disk preconditions; **enforcement is MS6**.
No step touches MS5 `describe` assembly/JSON-schema or MS6 `validate` rule logic. No
step reads a gridded data chunk (`c/0/0/0`) or a pixel raster — the gridded readers
read only Zarr array metadata + 1-D coordinate arrays + CF `grid_mapping`, and COG
tags + band metadata + georef (architecture §1); the no-pixel discipline is asserted
in S2/S3 tests (LOW-3). No reader works around a writer/reader mismatch: a Zarr
consolidated-metadata mismatch or a COG band-description mismatch is fixed by
**regenerating the MS2 fixture**, never by a reader hack (MED-4 / MED-5).

---

## Steps

### MS4-S1 — Shared gridded/geometry types, the single grid convention, and the R1 + MED-4 decision record

**Intent.** Stand up, with **zero new IO dependencies**, the typed vocabulary the
three readers (S2–S4) and the assembler (S5) all consume, and freeze the two
load-bearing decisions in one reviewable place **before** any reader is written:
(a) the single `GridExtent` cell-edge convention with the documented Zarr center→edge
half-pixel rule (the fix for the prior HIGH defect), and (b) the R1 crate choices +
MED-4 three-outcome COG-band protocol + §8 consolidated-metadata gate, recorded as an
architecture amendment. Independently committable: it compiles, adds pure types +
docs + tests, and leaves the repo green; no reader yet depends on it failing.

**Changes.**
- `crates/core/src/grid.rs` (new): the grid value types, all inert/agnostic, all
  fields private with getters, derive `Debug, Clone, PartialEq`:
  - `GridResolution { x_res: f64, y_res: f64 }` — per-axis signed resolution in CRS
    units (x_res > 0 marching east; y_res < 0 marching south, matching a north-up
    raster). Constructor only; no parsing of meaning.
  - `GridExtent { west: f64, north: f64, east: f64, south: f64 }` — the **north-west
    cell-EDGE origin** convention (GeoTIFF-native). Doc states the convention
    explicitly and the Zarr center→edge rule (`west = lon[0] − x_res/2`,
    `north = lat[0] + y_res/2`, signs per axis). A single associated constructor
    `from_edge_origin(west, north, res, width, height)` derives east/south so both
    readers build it identically.
  - `GridInfo { grid_label: GridLabel, extent: GridExtent, resolution: GridResolution,
    width: usize, height: usize, crs: Crs }` — per-grid-label representative geometry
    (architecture §3.5). Carries the recorded `Crs` (the `EPSG:<code>`-when-resolvable
    rule). No transform/role/semantic field.
  - A `GriddedField` is just MS1's [`Field`] with a `GriddedStatic` / `GriddedDynamic`
    quadrant and `Some(GridLabel)` — **no new field type** (reuse `Field`, honoring
    the §2 ordinary-field discipline). S1 adds only a doc note; the readers construct
    `Field`s.
  - A free helper `center_to_edge(first_center: f64, res: f64) -> f64` returning
    `first_center − res/2` (and its sign-aware use for north), unit-tested directly.
- `crates/core/src/error.rs`: add named-field `thiserror` variants, each doc-commented
  with *when* it fires (no `unwrap`/`expect`/panic in any reader): `ZarrRead { artifact,
  detail }`, `CogRead { artifact, detail }`, `GeoparquetRead { artifact, detail }`,
  `MissingGridGeoref { artifact, detail }` (CF `grid_mapping` / GeoTIFF georef tags
  absent — feeds G3), `MissingGriddedCoordinate { artifact, coordinate }` (a required
  1-D `lat`/`lon`/`time` coordinate array absent), `MissingGeometryColumn { artifact,
  column }` (outlines missing `basin_id` / `delineation` / `geometry`). All
  inert/agnostic (only artifact name + opaque detail/column). Extend
  `lib.rs::every_core_error_variant_constructs` to construct the new variants and bump
  its count.
- `crates/core/src/lib.rs`: `pub mod grid;` + module-map doc line.
- `architecture.md` **Amendments log** (append one dated entry, newest first) — MS4
  body text is not rewritten, only the amendments table grows:
  - **R1 (Zarr/COG/geometry) decided.** Pure-Rust stack, **no GDAL**: Zarr via the
    chosen Zarr-v3 crate (S2), COG via the pure-Rust `tiff` crate + manual GeoKey/tag
    parsing (S3), geoparquet via the **already-present** `parquet`/`arrow` stack reused
    for its schema + columns + `geo` KV (S4). Records the exact crate + major pinned in
    each of S2–S4 (filled in as each lands), and the contingency rule (LOW): *if a
    pinned crate's API differs at implementation time, pin the working adjacent major
    and record it here as a follow-up amendment, so a version surprise is a recorded
    pin-bump, never an ad-hoc red commit.* This contingency covers the Zarr crate, the
    `tiff` crate, and any decompressor, since crate APIs cannot be confirmed offline.
  - **The single `GridExtent` convention.** Cell-edge NW origin; Zarr center→edge
    half-pixel conversion; verified on the fixture (Zarr 10.125/49.875 centers →
    10.0/50.0 edges == COG tiepoint). States *why*: two genuinely-aligned artifacts
    must yield identical extents for the G2 precondition (S5) to be observable.
  - **The CRS-recording rule.** `EPSG:<code>` when an EPSG id resolves; raw string +
    R3 flag otherwise. Seeds MS6 M5; MS4 records, never cross-checks.
  - **MED-4 — COG band-description three-outcome protocol.** Decision is recorded
    here as a *named* protocol S3 executes against the fixture round-trip; the outcome
    is filled in when S3 lands:
    - **Outcome (1) — pure-Rust read works:** the `tiff` crate surfaces tag 42112
      `GDAL_METADATA` (ASCII) and HDX parses the small fixed `<GDALMetadata>` XML to
      pull `<Item ... role="description">…</Item>` (= field name) and
      `<Item name="units" …>` (= units). G1 COG-side is metadata-deep and **live**.
      (Ground truth: the fixture stores the band name in tag 42112, not IFD tag 270.)
    - **Outcome (2) — pure-Rust fails, GDAL accepted:** record the GDAL system-
      dependency cost as an explicit amendment **and** confirm the MS9 maturin/PyO3
      wheel still builds with it. (This step does not silently reintroduce GDAL.)
    - **Outcome (3) — pure-Rust fails, GDAL rejected:** G1 COG band-name verification
      is reported as an **R3 byte/format-deep SKIP-with-reason**, never silently
      claimed.
    - **Mismatch rule:** if the chosen reader cannot read the band descriptions the
      MS2 generator wrote, that is an **MS2 regenerate** (write the descriptions in a
      tag the reader supports), **never** a reader workaround.
  - **MED-5 — §8 consolidated-metadata gate.** S2 MUST confirm **from the Rust side**
    that the fixture's Zarr v3 store exposes its metadata via the §8 consolidated path
    (one read to learn the store). Either the reader reads it via the consolidated path
    (**live**), OR consolidated-metadata / v3-sharding verification is classified as an
    **R3 byte-deep SKIP with a stated reason** — documented, never silently claimed. A
    zarr-python vs Rust mismatch is fixed by **regenerating the fixture**, never a
    reader workaround. (Ground truth: `consolidated_metadata.kind == "inline"` in the
    root `zarr.json`, all six members present.)
  - **LOW-3 — no-gridded-chunk-decode review gate.** Records that no gridded-chunk
    decode happens anywhere in `hdx-core`: the gridded readers read only Zarr array
    metadata + 1-D `lat`/`lon`/`time` arrays + CF `grid_mapping`, and COG tags/band
    metadata/georef — **never** `c/` chunk payloads or pixel rasters; the `gridded_*`
    subtrees are opaque leaves to the layout walk and metadata-only to the readers.

**Test plan.**
- `grid.rs` unit: `center_to_edge(10.125, 0.25) == 10.0` and `lat[0]=49.875` →
  `north = 49.875 + 0.25/2 == 50.0` (the half-pixel rule, the prior-defect fix, pinned
  in code).
- `grid.rs` unit: `GridExtent::from_edge_origin(10.0, 50.0, 0.25, width=6, height=8)`
  yields `east == 11.5`, `south == 48.0` (matches the decoded COG bounds), and a
  round-trip `GridInfo` constructs with `Crs::new("EPSG:4326")`.
- `grid.rs` unit: a `GriddedField` is an ordinary `Field` (gridded quadrant +
  `Some(GridLabel)`); a name like `era5_precipitation_was_filled` carries no magic
  (constructs verbatim).
- `lib.rs`: every new `CoreError` variant constructs and renders a non-empty `Display`;
  count updated.
- A `grid.rs` module doc line + S1 doc note stating the **no-pixel / no-chunk** gate
  (LOW-3), referenced by S2/S3 tests.

**Acceptance.**
- `cargo build` + `cargo test` + `cargo clippy --all-targets -- -D warnings` pass.
- `GridExtent` documents the single cell-edge convention and the Zarr center→edge
  rule; the half-pixel math is asserted in a test (`10.125 → 10.0`, `49.875 → 50.0`).
- New `thiserror` variants are named-field, doc-commented with *when*, inert/agnostic;
  no `unwrap`/`expect`/`panic` introduced.
- `architecture.md` Amendments log carries the dated MS4 entry recording: R1 decision +
  LOW contingency, the GridExtent convention, the CRS rule, the MED-4 three-outcome
  protocol, the MED-5 consolidated-metadata gate, and the LOW-3 no-chunk gate.
- Advances (foundations, enforced MS6): the **G2** alignment precondition's type model;
  **G1**/**G3**/**Geo1**/**M5** representation scaffolding.
- Commit via `./scripts/bump-version.sh patch` → stage `Cargo.toml` → conventional
  commit → `git tag v<version>`.

**Spec refs.** §1 (inert/agnostic), §2 (ordinary fields, self-naming), §7 (per-variable
native grids; one dataset-wide CRS), §8 (shared grid label ⇒ alignment), §9
(delineation), §11 (six-field floor untouched), §14 G1/G2/G3/Geo1/M5 (foundations);
architecture §1, §3.5, §7 R1/R3, §8 Amendments.

**Commit message.** `feat(core): add grid types + single edge convention; record MS4 R1/MED-4/MED-5 decisions`

---

### MS4-S2 — Zarr v3 reader: consolidated metadata, 1-D coordinates, CF georef (MED-5 + the half-pixel fix)

**Intent.** Read the `gridded_dynamic/<label>.zarr` store **metadata-only** into the
gridded·dynamic field catalog + a `GridInfo`, confirming **from the Rust side** that
the store is learned via the §8 **consolidated-metadata** path (MED-5), and applying
S1's center→edge conversion so the Zarr extent equals the COG extent (the prior-defect
fix). No `c/0/0/0` data chunk is ever read (LOW-3).

**Changes.**
- `crates/core/Cargo.toml`: add the pure-Rust Zarr-v3 metadata crate (R1), pinned to a
  major, `default-features` trimmed to metadata + local-filesystem reads (no async, no
  object_store, no cloud). Crate + version recorded into the S1 architecture amendment.
  **LOW contingency:** if the chosen crate's metadata API differs from what is assumed
  at implementation time (it cannot be confirmed offline), pin the working adjacent
  major and append a one-line follow-up amendment — a version surprise is a recorded
  pin-bump, not an ad-hoc change. If the crate cannot read the **inline consolidated
  metadata** at all, fall back to reading the root `zarr.json` directly with
  `serde_json` (the consolidated metadata is plain JSON in the root object — see ground
  truth) and record that as the MED-5 path taken; this keeps the no-network, no-GDAL
  guarantee.
- `crates/core/src/zarr_reader.rs` (new):
  - `read_zarr_grid(path, grid_label) -> Result<ZarrGrid, CoreError>` opening the store
    **via the consolidated-metadata path** (one read of the root `zarr.json`'s
    `consolidated_metadata.metadata` map; ground truth: `kind == "inline"`, six
    members). Records which path was taken (`ConsolidatedMetadataSource::Consolidated`
    vs an `R3Skip` reason) as a self-documenting enum, never a bool.
  - **Array classification (tightened per the LOW critique):** an array is a
    **coordinate** iff its name is one of `time`/`lat`/`lon` (it self-references that
    dimension via `dimension_names`); it is a **data variable** (gridded·dynamic field)
    iff it carries a `grid_mapping` attribute and 3-D `dimension_names`
    `[time, lat, lon]`; the **grid_mapping target** (`crs`) is resolved **exclusively
    by following a data variable's `grid_mapping` attribute** — *not* by dimension
    self-reference (the `crs` array has `shape: []` and **no** `dimension_names`, so it
    is unreachable by dimension grouping; ground truth). The `crs` array is read only
    after a data var points at it.
  - Reads the 1-D `lat`/`lon`/`time` coordinate arrays (their single chunk `c/0`, a 1-D
    coordinate read — **architecture §1**), the CF `units` per data variable, the CRS
    from the `grid_mapping` target's `spatial_ref` / `crs_wkt` attrs recorded as
    `EPSG:<code>` (S1 rule), and builds `GridInfo` with `GridExtent::from_edge_origin`
    after `center_to_edge` (so Zarr west=10.0 / north=50.0).
  - Maps each data variable (e.g. `era5_precipitation`, `era5_precipitation_was_filled`)
    to an ordinary `GriddedDynamic` [`Field`] with `Some(GridLabel)`, dtype via MS1
    `parse_dtype` over the Zarr `data_type` string, units from the CF `units` attr —
    **no name-pattern special-casing** (§2).
  - **NEVER opens a `c/0/0/0` data chunk** — only `zarr.json` + the 1-D coord chunks.
    `#[instrument]` on the public fn; `tracing` only.

**Test plan.** All against `conformance/valid/minimal/basin=0001/gridded_dynamic/era5.zarr`:
- **Raw center values pinned (makes the half-pixel step visible — prior-defect fix):**
  the decoded `lon[0] == 10.125` and `lat[0] == 49.875` (cell centers), `res == 0.25`,
  `width == 6`, `height == 8`.
- **Converted edge extent pinned:** the resulting `GridInfo.extent` is
  `west == 10.0`, `north == 50.0`, `east == 11.5`, `south == 48.0` — the bytes and the
  formula now agree.
- **MED-5 consolidated path is live:** a test asserts the reader learned the store via
  `ConsolidatedMetadataSource::Consolidated` (the inline path), and that all six members
  were enumerated from that single read. If the crate cannot do this and the
  `serde_json` root-`zarr.json` fallback is used, the test asserts the recorded source
  is still the consolidated path; only if neither works is the source an `R3Skip` with
  a stated reason (asserted), never silently claimed.
- **G1 self-naming:** the two data variables are catalogued as ordinary fields named
  exactly `era5_precipitation` and `era5_precipitation_was_filled`, quadrant
  `GriddedDynamic`, `grid_label == era5`, no positional channel axis; the
  `{source}_{variable}` + companion-mask names get no special handling.
- **G3 CF georef present:** the `grid_mapping` target `crs` is resolved by following
  the data var's `grid_mapping` attr (not by dimensions), and its CRS records as
  `EPSG:4326`.
- **CF units:** `era5_precipitation` records `units == "mm"`.
- **LOW-3 no-chunk gate:** a test deletes/renames the `era5_precipitation/c/0/0/0`
  shard file in a *temp copy* of the store and asserts `read_zarr_grid` still succeeds
  and returns identical metadata + extent — proving no data chunk is read. (Coordinate
  chunks `lat/c/0`, `lon/c/0`, `time/c/0` are kept; ground truth confirms data chunks
  are deletable while coord chunks are needed.)
- Negative: a store whose data var has no `grid_mapping` target → `MissingGridGeoref`;
  a store missing the `lon` coordinate array → `MissingGriddedCoordinate`.

**Acceptance.**
- `cargo build` + `cargo test` + `cargo clippy --all-targets -- -D warnings` pass.
- The Zarr `GridInfo` extent is **edge-based 10.0/50.0** (matching the COG), via S1's
  documented center→edge conversion; raw centers 10.125/49.875 are separately asserted.
- **MED-5 satisfied:** the store is read via the §8 consolidated-metadata path and that
  path is recorded as live (or, only if genuinely unreadable, an R3 skip-with-reason is
  recorded and asserted — never silently claimed). The Rust-side confirmation exists.
- **LOW-3 satisfied:** a test proves no `c/0/0/0` data chunk is read.
- R1 Zarr crate + pin recorded in the S1 architecture amendment; the LOW contingency is
  stated.
- Advances (foundations, enforced MS6): **G1** (self-naming Zarr variables, no channel
  axis), **G3** (CF `grid_mapping`/CRS present), the **G2** precondition (per-grid
  `GridInfo` for the shared label), **M5** (per-file CRS read as `EPSG:4326`).
- Commit via `./scripts/bump-version.sh patch` → stage `Cargo.toml` → conventional
  commit → `git tag v<version>`.

**Spec refs.** §7 (CF georef; per-variable native grid; one CRS), §8 (Zarr v3; v3
sharding; consolidated metadata; self-naming variables), §1/§2 (inert; ordinary
fields); §14 G1/G3/G2-precondition/M5 (foundations); architecture §1, §3.5, §7 R1/R3,
MED-5.

**Commit message.** `feat(core): add Zarr v3 metadata reader via consolidated path with center-to-edge extent`

---

### MS4-S3 — COG reader: band descriptions + standard georef tags (MED-4 three-outcome protocol)

**Intent.** Read the `gridded_static/<label>.tif` COG **metadata-only** into the
gridded·static field catalog + a `GridInfo`, executing the **MED-4 three-outcome
protocol** to read per-band descriptions (= field names) from the fixture, and reading
the standard GeoTIFF georef tags into an edge-based `GridInfo` that matches S2's Zarr
extent. No pixel raster is ever decoded (LOW-3).

**Changes.**
- `crates/core/Cargo.toml`: add the pure-Rust `tiff` crate (R1), pinned to a major,
  features trimmed to tag reading (no image-decode features needed; we read tags, not
  rasters). Crate + version recorded into the S1 architecture amendment. **LOW
  contingency:** if the pinned `tiff` major's tag-reading API differs from what is
  assumed (it cannot be confirmed offline), pin the working adjacent major and append a
  one-line follow-up amendment — a recorded pin-bump, not an ad-hoc red commit.
- `crates/core/src/cog_reader.rs` (new):
  - `read_cog_grid(path, grid_label) -> Result<CogGrid, CoreError>` opening the TIFF and
    reading **tags only**:
    - **Band description (MED-4):** read tag **42112 `GDAL_METADATA`** as an ASCII
      string and parse the small fixed `<GDALMetadata>` XML for
      `<Item ... role="description">NAME</Item>` (= field name) and
      `<Item name="units" …>UNIT</Item>` (= units). (Ground truth: the fixture stores
      the band name here, not in IFD tag 270.) The XML parse is a minimal,
      dependency-free substring/attribute extraction — HDX parses only the two Items it
      needs and treats the value as an opaque producer string (§2). Records a
      `CogBandSource` enum (`GdalMetadataTag` vs an `R3Skip` reason).
    - **MED-4 outcome handling:**
      - **(1) success (expected):** the description reads back as `elevation` → G1
        COG-side is metadata-deep and live.
      - **(2)/(3):** if the `tiff` crate cannot surface tag 42112, the step does **not**
        silently claim G1; it records the band source as `R3Skip("tiff cannot read tag
        42112 …")` (outcome 3) — and the architecture amendment's outcome-(2) branch
        (accept GDAL, confirm MS9 wheel) is invoked only by an explicit follow-up, never
        silently. The **mismatch rule** is honored: if the reader cannot read what MS2
        wrote, the fix is an **MS2 regenerate** (write the description in a tag the
        reader supports), not a reader hack. The round-trip test below is what proves
        which outcome holds.
    - **Georef (G3):** read `ModelPixelScale` (33550) for `GridResolution`,
      `ModelTiepoint` (33922) for the NW cell-edge origin (already edge-based — no
      conversion), `ImageWidth`/`ImageLength` for `width`/`height`, and the EPSG code
      from `GeoKeyDirectory` (34735) / `GeoAsciiParams` (34737) recorded as
      `EPSG:<code>` (S1 rule). Builds `GridInfo` via `GridExtent::from_edge_origin` →
      west=10.0 / north=50.0, east=11.5, south=48.0.
  - Maps the band to an ordinary `GriddedStatic` [`Field`] named `elevation`,
    `Some(GridLabel)`, dtype via MS1 `parse_dtype` over the TIFF `SampleFormat`+bit
    depth (`float32` → `f32`), units `m` from the same XML.
  - **NEVER decodes a pixel raster / tile** — reads tags only. `#[instrument]`;
    `tracing` only; no `unwrap`/`expect`/`panic`.

**Test plan.** All against `conformance/valid/minimal/basin=0001/gridded_static/era5.tif`:
- **MED-4 round-trip (the decision):** the band description reads back as exactly
  `elevation` from tag 42112, and `CogBandSource == GdalMetadataTag` (outcome 1 — live).
  This is the executable proof the pure-Rust read works on the real fixture; if it ever
  regresses, the recorded protocol routes to outcome (2)/(3), never a silent claim.
- **Band units:** the band records `units == "m"` (from the same tag-42112 XML).
- **G3 georef:** the `GridInfo` resolution is `x_res == 0.25`, `y_res == −0.25`,
  `width == 6`, `height == 8`, CRS `EPSG:4326`.
- **Edge extent matches Zarr (couples to S5):** `GridInfo.extent` is `west == 10.0`,
  `north == 50.0`, `east == 11.5`, `south == 48.0` — byte-identical to S2's converted
  Zarr extent (the prior-defect fix realized end-to-end).
- **G1 self-naming:** the band is one ordinary field named verbatim `elevation`,
  quadrant `GriddedStatic`, no positional channel axis.
- **LOW-3 no-pixel gate:** a test asserts the reader returns the metadata without
  decoding pixels — e.g. it succeeds on the tags even when the read path never requests
  a strip/tile, and (where the crate API allows) a test confirms no decode call is made;
  at minimum the public API exposes no pixel buffer and the doc + review gate assert
  tags-only.
- Negative: a TIFF with no georef tags → `MissingGridGeoref`; an unmappable
  `SampleFormat` → `UnknownDtype`.

**Acceptance.**
- `cargo build` + `cargo test` + `cargo clippy --all-targets -- -D warnings` pass.
- **MED-4 resolved with a recorded outcome:** the fixture round-trip proves outcome (1)
  (band name `elevation` read from tag 42112 via pure Rust); the architecture amendment
  records the outcome. G1 COG-side is live, not silently claimed. If the round-trip
  fails, the band source is an R3 skip-with-reason (outcome 3) and the mismatch is
  flagged as an MS2 regenerate — never a reader workaround.
- The COG `GridInfo` extent is edge-based 10.0/50.0 (no conversion needed) and equals
  S2's Zarr extent.
- R1 `tiff` crate + pin recorded in the S1 amendment; LOW contingency stated.
- Advances (foundations, enforced MS6): **G1** (band description = field name, no
  channel axis), **G3** (GeoTIFF georef tags present), the **G2** precondition (COG
  `GridInfo` for the shared label), **M5** (per-file CRS `EPSG:4326`).
- Commit via the bump+tag convention.

**Spec refs.** §7 (GeoTIFF georef tags; per-variable native grid; one CRS), §8 (COG =
one grid; band description = field name; internal tiling + overviews), §1/§2 (inert;
ordinary fields); §14 G1/G3/G2-precondition/M5 (foundations); architecture §1, §3.5,
§7 R1/R3, MED-4.

**Commit message.** `feat(core): add pure-Rust COG reader for band descriptions and georef tags`

---

### MS4-S4 — geoparquet reader: schema, delineation/basin_id columns, and CRS

**Intent.** Read `outlines.geoparquet` **metadata + 1-D columns only** into the
delineation list + the structural facts Geo1/I1 need, reusing MS3's pure-Rust
`parquet`/`arrow` stack (R1 — no new crate), and recording the CRS as a comparable
`EPSG:<code>` (the MEDIUM critique fix) so MS6's M5 receives a value it can compare to
the manifest's `"EPSG:4326"`.

**Changes.**
- `crates/core/src/geoparquet_reader.rs` (new):
  - `read_outlines(path) -> Result<OutlinesInfo, CoreError>` reusing
    `read_parquet_meta` (the existing private touchpoint) for the schema, then a bounded
    1-D read of the `delineation` and `basin_id` columns (the same bounded key-column
    pattern MS3 already uses — never the `geometry` blob, never a chunk):
    - **Schema check (Geo1):** records presence of the three required columns
      `basin_id`, `delineation`, `geometry`; missing any → `MissingGeometryColumn`.
    - **`delineation` labels:** the distinct values read into `Vec<DelineationLabel>`
      (ground truth: `{grit, merit}`), opaque producer strings (§9 — no "hydrofabric"
      assumption).
    - **`basin_id` (I1 input):** presence recorded (the outlines `basin_id` column the
      MS6 I1 check needs); the bounded read confirms it is a real column.
    - **"Not partitioned by delineation" (Geo1):** a structural fact — `outlines` is a
      **single file at the dataset root** (the layout walk already records this as a
      root rollup), not a `delineation=<x>/` hive. Records this as a boolean fact read
      from the layout, never decided here.
    - **CRS (the MEDIUM fix — S4 says precisely what `Crs` holds):** read the parquet
      `geo` KV metadata, take `primary_column`'s `crs`. The fixture's CRS is a
      **PROJJSON object** with `id == {authority:"EPSG", code:4326}` (NOT the string
      `"EPSG:4326"`). The reader extracts `id.authority`+`id.code` and records
      `Crs::new("EPSG:4326")` — a value MS6's M5 can compare to the manifest. **If the
      PROJJSON has no `id`** (or `authority != "EPSG"`), the reader records the raw
      PROJJSON string verbatim and flags that file's M5-readiness as an **R3** item in
      the returned facts (documented, never silently claimed). Parsing the `geo` KV +
      PROJJSON uses `serde_json` (already a dependency).
  - `OutlinesInfo` is inert/agnostic: `delineations: Vec<DelineationLabel>`,
    `has_basin_id: bool`, `has_geometry: bool`, `partitioned_by_delineation: bool`,
    `crs: Crs`, plus a `crs_source` enum (`EpsgFromProjjsonId` vs `RawProjjsonR3`) — no
    geometry payload, no transform/role/provenance. `#[instrument]`; `tracing` only.

**Test plan.** All against `conformance/valid/minimal/outlines.geoparquet`:
- **Geo1 schema:** the three columns `basin_id`, `delineation`, `geometry` are present;
  `partitioned_by_delineation == false` (single root file).
- **Delineation labels:** `delineations` == `{grit, merit}` (order-insensitive), each an
  opaque `DelineationLabel`.
- **I1 input:** `has_basin_id == true` and the bounded `basin_id` read returns the
  expected ids `{0001, 0002, 0003}` (4 rows, basin 0001 carries both delineations →
  §9 plurality, asserted).
- **CRS (the MEDIUM fix asserted):** `crs == Crs::new("EPSG:4326")` and
  `crs_source == EpsgFromProjjsonId` — proving the reader resolved `EPSG:4326` from the
  PROJJSON `id`, handing MS6's M5 a comparable value (not the raw PROJJSON blob).
- A unit test over a synthetic `geo` KV whose CRS PROJJSON **lacks `id`** asserts the
  reader records the raw string + `crs_source == RawProjjsonR3` (the documented R3
  fallback), never panicking.
- Negative: an outlines parquet missing the `delineation` column →
  `MissingGeometryColumn`.

**Acceptance.**
- `cargo build` + `cargo test` + `cargo clippy --all-targets -- -D warnings` pass.
- **The MEDIUM CRS issue is fixed:** `Crs` on the geoparquet path holds `EPSG:4326`
  (resolved from PROJJSON `id`), asserted on the fixture; the no-`id` R3 fallback is
  tested. MS6's M5 receives a comparable value.
- No new dependency (reuses the `parquet`/`arrow`/`serde_json` already present);
  geoparquet flavor read structurally (no dependence on optional geo-metadata blocks
  beyond the `geo` KV the spec implies).
- The `geometry` column is never decoded — only schema + the `basin_id`/`delineation`
  1-D columns are read.
- Advances (foundations, enforced MS6): **Geo1** (`(basin_id, delineation, geometry)`
  schema; `delineation` label column; not partitioned), **I1** (`basin_id` in
  outlines), **M5** (per-file CRS as `EPSG:4326`).
- Commit via the bump+tag convention.

**Spec refs.** §9 (plural outlines; `delineation` column; not partitioned;
delineation-neutral), §3 (`basin_id` authoritative), §7.4/§11 (one CRS), §1/§2 (inert);
§14 Geo1/I1/M5 (foundations); architecture §1, §3.5, §7 R1/R3.

**Commit message.** `feat(core): add geoparquet reader for delineation/basin_id and EPSG CRS`

---

### MS4-S5 — Assemble the gridded/geometry half and complete the combined discovery model

**Intent.** Compose S2+S3+S4 into the gridded/geometry half of the shared discovery
layer and pair it with MS3's `ScalarDiscovery`, so both verbs (MS5/MS6) consume **one**
typed in-memory model. This is where the **G2 alignment precondition is observed**
(read, not enforced): the same grid label appears across the `gridded_static` (COG) and
`gridded_dynamic` (Zarr) subtrees, **and** the two `GridInfo` extents now **coincide**
at 10.0/50.0 — byte-true because of S1's single convention (the prior HIGH defect's
end-to-end fix). MS4 ends with the discovery layer complete.

**Changes.**
- `crates/core/src/gridded_discovery.rs` (new) — the gridded/geometry assembler,
  mirroring `discovery.rs`'s "records facts, never a verdict" discipline:
  - `discover_gridded(path) -> Result<GriddedDiscovery, CoreError>` walks the
    `LayoutModel` (reusing `walk_layout`), and for each basin with a present
    `gridded_static` / `gridded_dynamic` subtree, finds the `<label>.tif` / `<label>.zarr`
    artifacts and calls `read_cog_grid` (S3) / `read_zarr_grid` (S2); reads
    `outlines.geoparquet` once via `read_outlines` (S4).
  - `GriddedDiscovery` (inert/agnostic): `grids: Vec<GridInfo>` (per grid label,
    representative — the COG `GridInfo` and the Zarr `GridInfo` for the shared `era5`
    label), `gridded_fields: Vec<Field>` (the homogeneous gridded catalog: `elevation`
    GriddedStatic + `era5_precipitation` & `…_was_filled` GriddedDynamic),
    `delineations: Vec<DelineationLabel>`, and the per-basin observed grid labels across
    the two subtrees (the **G2 precondition fact**). No verdict; gaps surfaced as facts
    (a scalar-only basin with no gridded subtree records empty gridded facts).
  - A `Discovery` struct (or equivalent) that **pairs** `ScalarDiscovery` (MS3) with
    `GriddedDiscovery` without reshaping either — the unified model architecture §3.5
    describes (`basins`, `fields` = scalar ⊕ gridded, `grids`, `time_extent`,
    `delineations`). Its accessors are pinned by S5 so MS5 cannot silently reshape it.
  - `crates/core/src/lib.rs`: `pub mod gridded_discovery;` + module-map doc.
  - `crates/core/README.md`: extend the Mermaid module map + glossary with the gridded
    half (`grid`, `zarr_reader`, `cog_reader`, `geoparquet_reader`, `gridded_discovery`)
    and glossary terms (GridExtent cell-edge convention, GridInfo, consolidated
    metadata, shared grid label ⇒ alignment, delineation). Crate README is the agent
    entry point (CLAUDE.md docs rule for the complex crate).

**Test plan.** All against `conformance/valid/minimal/`:
- **G2 precondition observed (the prior HIGH defect's end-to-end fix):** for basin 0001
  the COG `<label>` and the Zarr `<label>` are both `era5` (shared label across
  subtrees), **and** the COG `GridInfo.extent` equals the Zarr `GridInfo.extent`
  (`west==10.0, north==50.0, east==11.5, south==48.0`) and resolutions/dims coincide.
  This assertion now passes because both readers produce edge-based extents — two
  genuinely-aligned artifacts look aligned. A comment marks this as the **G2
  precondition** (observed, not enforced — enforcement is MS6).
- **Gridded field catalog:** exactly `{elevation: GriddedStatic, era5_precipitation:
  GriddedDynamic, era5_precipitation_was_filled: GriddedDynamic}`, all carrying
  `grid_label == era5`, names verbatim (no `{source}_{variable}` / companion-mask
  magic — §2).
- **G3:** every grid's CRS records `EPSG:4326`; CF (`grid_mapping`) and GeoTIFF georef
  presence both observed.
- **Geo1 + delineations:** `delineations == {grit, merit}`; outlines schema facts
  surfaced.
- **Combined model:** `Discovery` exposes the scalar half unchanged (the four MS3
  `ScalarDiscovery` accessors still pass) and the gridded half alongside it; a seam test
  pins both halves' accessors so MS5 builds on a stable shape.
- **MED-5 surfaced at the assembler level:** the combined model records which Zarr path
  was taken (consolidated/live or R3 skip), so MS5/MS6 can report it.
- Gaps-as-facts: discovering a tree where a basin lacks a gridded subtree succeeds and
  records empty gridded facts for that basin (no verdict).

**Acceptance.**
- `cargo build` + `cargo test` + `cargo clippy --all-targets -- -D warnings` pass.
- **The G2 on-disk precondition is byte-true and asserted:** the Zarr and COG
  `GridInfo` extents **coincide** at 10.0/50.0 on the fixture — the structural-misread
  defect is gone; MS6 receives a true G2 signal, not a manufactured-false one.
- The discovery layer is **complete**: a single `Discovery` model pairs the scalar
  (MS3) and gridded/geometry (MS4) halves for MS5/MS6, with no reshaping of
  `ScalarDiscovery`.
- The combined model records the MED-5 Zarr-path classification (consolidated/live or
  R3 skip) for honest downstream reporting.
- Advances (foundations, enforced MS6): **G1** (self-naming across both subtrees),
  **G2** precondition (shared label + coinciding extents observed), **G3** (CF +
  GeoTIFF georef present), **Geo1** (outlines schema + `delineation`), **I1**
  (`basin_id` in outlines).
- `crates/core/README.md` updated with the full module map (the complex-crate doc
  requirement).
- Commit via the bump+tag convention.

**Spec refs.** §4 (basin-first hive; the gridded subtrees), §7 (per-variable native
grids; one CRS), §8 (one artifact = one grid; **shared grid label ⇒ cell-for-cell
alignment**; self-naming), §9 (plural outlines; `delineation`), §5 (one-basin
homogeneous catalog), §1/§2 (inert; ordinary fields); §14 G1/G2-precondition/G3/Geo1/I1
(foundations); architecture §1, §3.5, §5, §7 R1/R3.

**Commit message.** `feat(core): assemble gridded/geometry discovery half with coinciding G2 extents`

---

## Coverage map — every MS4 deliverable / exit criterion / spec ref is assigned

| MS4 deliverable / exit criterion (milestones.md) | Step(s) |
|---|---|
| R1 decision (Zarr/COG/geometry) recorded as architecture amendment | S1 (recorded) + S2/S3/S4 (crate + pin filled in) |
| Zarr reader: per-grid metadata, `time`/`lat`/`lon` coords, CF `grid_mapping`/CRS, units, extent/affine/res | S2 |
| §8 consolidated-metadata gate (live via consolidated path **or** R3 skip-with-reason) — MED-5 Rust-side confirmation | S2 (decision recorded S1) |
| v3-sharding verification classification | S1 (gate) + S2 (recorded with the consolidated decision) |
| COG reader: band descriptions (= field names), georef tags, extent/affine/res, units — MED-4 three-outcome protocol | S3 (decision recorded S1) |
| geoparquet reader: `(basin_id, delineation, geometry)` schema, `delineation`, `basin_id`, not-partitioned | S4 |
| CRS recorded consistently / comparably for M5 (the MEDIUM fix) | S1 (rule) + S2/S3 (EPSG from files) + S4 (EPSG from PROJJSON `id`) |
| Output feeds the unified discovery model alongside `ScalarDiscovery` | S5 |
| Shared grid label observed across static/dynamic subtrees (G2 precondition) | S5 (extents coincide via S1 convention) |
| Discovery layer complete (both verbs can assemble) | S5 |
| LOW-3 no-gridded-chunk / no-pixel review gate | S1 (gate doc) + S2 (no `c/0/0/0`) + S3 (no pixel decode) |
| Spec G1 (self-naming, no channel axis) | S2 + S3 (+ S5 across subtrees) |
| Spec G2 precondition (shared label ⇒ alignment, MUST exhibit) | S5 |
| Spec G3 (CF / GeoTIFF georef present) | S2 (CF) + S3 (GeoTIFF) |
| Spec Geo1 (outlines schema + `delineation`) | S4 |
| Spec I1 (basin_id in outlines) | S4 |
| Spec M5 (per-file CRS read; rule is MS6) | S2 + S3 + S4 |
| Every step: build + test + clippy `-D warnings` + bump+tag | S1–S5 |

**Note on the LOW crate-version contingency.** S2 and S3 each carry an explicit
acceptance line: if the pinned Zarr / `tiff` crate's API differs at implementation time
(unverifiable offline), pin the working adjacent major and record it as a follow-up
architecture amendment — a version surprise becomes a recorded pin-bump, never an
ad-hoc red commit. S4 reuses the already-proven `parquet`/`arrow`/`serde_json` stack, so
it carries no new-crate risk.
