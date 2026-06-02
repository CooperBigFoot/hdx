# MS4 — Gridded + geometry metadata readers (discovery layer, gridded/geometry half) — STEP PLAN

> **Milestone:** MS4 (the fourth milestone of HDX v0.1; depends on MS3, which built the
> scalar half of the shared discovery layer + the layout walk).
> **Source contract:** `spec/HDX_SPEC.md` (canonical, settled).
> **Planned against:** `architecture.md` §1 (read metadata, not chunks), §3 (the type
> model / discovery types), §5 (verb responsibilities), §7 (R1 reader selection / R3
> depth), §8 (Amendments log), and `planning/milestones.md` (MS4 goal, deliverables,
> reviewable outcome, exit criteria, spec refs, risks).
>
> **Why this milestone is MS4.** MS1 (type model + manifest parse + manifest JSON
> Schema), MS2 (the dev-only Python fixture generator: one valid four-quadrant dataset
> + two minimal invalids under `conformance/`), and MS3 (the **scalar half** of the
> shared discovery layer — the layout walk + scalar-parquet metadata reader + the
> `discover_scalar` assembler) are built and green. MS4 completes the shared discovery
> layer by adding the **gridded + geometry half**: a Zarr v3 metadata reader, a COG
> metadata reader, and a geoparquet metadata reader, then a `discover_gridded` /
> combined-discovery seam alongside MS3's `ScalarDiscovery`. After MS4 both verbs
> (`describe` MS5, `validate` MS6) can assemble from one typed in-memory model.

---

## Ground-truth verification (done before this plan; pins every uncertain claim)

Every load-bearing on-disk and crate claim below was verified against the committed
MS2 fixture and by a throwaway Rust spike before writing this plan:

- **Zarr (MED-5).** `valid/minimal/basin=000{1,2,3}/gridded_dynamic/era5.zarr/zarr.json`
  carries **inline `consolidated_metadata`** (`"kind": "inline"`) holding every member's
  metadata (`crs`, `era5_precipitation`, `era5_precipitation_was_filled`, `lat`, `lon`,
  `time`). It is plain JSON — readable with `serde_json`, **no `zarrs` crate needed for
  metadata**. Data arrays carry `dimension_names: ["time","lat","lon"]`,
  `data_type`, and CF `attributes` (`units`, `grid_mapping: "crs"`, `_ARRAY_DIMENSIONS`).
  The `crs` member is a scalar (`shape: []`) `int32` array whose attributes hold
  `grid_mapping_name`, `crs_wkt`, and **`spatial_ref: "EPSG:4326"`**. Data arrays use the
  `sharding_indexed` codec; coordinate arrays (`lat`/`lon`/`time`) use `bytes`+`zstd`.
- **HIGH (int8).** `era5_precipitation_was_filled` has `data_type: "int8"` — **not in
  MS1's closed `Dtype` {F32,F64,I32,I64,Bool,Timestamp}**. This is a conformant mask
  (spec §12), so it is a **closed-enum extension** (`Dtype::I8`), NOT an MS2-regenerate.
- **COG (MED-4).** `gridded_static/era5.tif` is a tiled single-band GeoTIFF. The band
  description lives in **tag 42112 (GDAL_METADATA)** as an XML blob:
  `<Item name="DESCRIPTION" sample="0" role="description">elevation</Item>` and
  `<Item name="units" sample="0">m</Item>`. **Tag 270 (ImageDescription) is ABSENT.**
  `SampleFormat=3` (IEEE float) + `BitsPerSample=32` → `F32`. Georef tags present:
  `GeoKeyDirectory(34735)`, `GeoDoubleParams(34736)`, `GeoAsciiParams(34737="WGS 84|")`,
  `ModelPixelScale(33550=[0.25,0.25,0])`, `ModelTiePoint(33922=[0,0,0,10,50,0])`.
  **Spike result:** the pure-Rust `tiff` crate **at version 0.11** reads ALL of these via
  the typed vec accessors — `get_tag_ascii_string(Tag::Unknown(42112))` (band-desc XML),
  `get_tag_u32_vec(Tag::SampleFormat)`/`(Tag::BitsPerSample)`,
  `get_tag_u16_vec(Tag::Unknown(34735))`, `get_tag_f64_vec(Tag::Unknown(33550|33922))`,
  `get_tag_ascii_string(Tag::Unknown(34737))`. **MED-4 outcome (1): pure-Rust works, no
  GDAL.** Caveat recorded below: `tiff 0.9`'s `find_tag`/`get_tag(Value)` returns `None`
  for unknown geo tags; **pin `tiff = "0.11"` and use the typed `get_tag_*_vec`
  accessors** — do NOT use `find_tag` for the geo tags.
- **Geoparquet.** `outlines.geoparquet` has columns **exactly `(basin_id, delineation,
  geometry)`**; `geometry` is arrow `binary` (geoarrow.wkb extension), `delineation`
  values `["merit","merit","merit","grit"]` (plural; `grit` for basin `0001`),
  `basin_id` `["0001","0002","0003","0001"]`, single non-partitioned root file, 1 row
  group, 4 rows. **Readable via the existing `parquet`/`arrow` metadata path** (schema +
  the bounded 1-D `delineation`/`basin_id` column reads MS3 already uses) — **no new
  geometry crate is required** for the metadata MS4 needs.

---

## Scope guard

Every step below stays strictly inside MS4 (milestones.md MS4 / architecture §1):

- **Metadata + 1-D coordinate reads only — NEVER gridded chunks or pixel rasters
  (architecture §1; folded LOW-3).** The gridded readers read **only**: Zarr **array
  metadata** (from the consolidated root `zarr.json`) + the **1-D `lat`/`lon`/`time`
  coordinate arrays** + the CF `grid_mapping`/CRS; and COG **tags / band metadata +
  georef**. They **never** read a gridded data array's `c/` chunk payload
  (`era5_precipitation/c/...`, `era5_precipitation_was_filled/c/...`) and never decode a
  COG strip/tile pixel raster. The `gridded_*` data variables are **opaque leaves** to
  the layout walk and **metadata-only** to the readers. (Reading the small 1-D
  coordinate arrays is the architecture-§1-sanctioned "Zarr `lat`/`lon` coord arrays"
  read — distinct from a `[T,Y,X]` data-chunk decode. The Zarr-side no-data-chunk
  guarantee is exercised by an explicit S2 test; the COG-side no-pixel guarantee by an
  explicit S3 test — both **mandatory**, not "where feasible".)
- **Discovery only, no enforcement.** MS4 *reads and models* the gridded + geometry
  half; it **enforces no spec §14 check**. It surfaces facts (per-grid `GridInfo`,
  gridded field catalog, CF/GeoTIFF georef presence, the shared-grid-label observation,
  `delineation` labels). The conformance verdict (G1–G3, Geo1, I1, M5, H2) is **MS6**,
  which runs rules over this model. The shared grid label across the static/dynamic
  subtrees is **observed and recorded** (G2 precondition), never asserted-equal here.
- **CRS read verbatim — MS4 interprets nothing.** The Zarr CRS is recorded **verbatim
  from the `crs` member's `spatial_ref` attribute** (`"EPSG:4326"`); the COG CRS is
  recorded **verbatim from the georef tags** (the resolved EPSG/GeoAscii string). MS4 does
  **not** parse/normalize `crs_wkt`, and does **not** compare CRS strings to the manifest —
  the spec-check **M5** cross-check is **MS6's** rule. MS4 only *reads* each file's CRS as
  an opaque `Crs` newtype.
- **Gridded + geometry half only.** MS4 reads each basin's `gridded_static/<label>.tif`,
  `gridded_dynamic/<label>.zarr`, and the root `outlines.geoparquet`. The scalar half
  (`scalar_static.parquet`, each `scalar_dynamic.parquet`) and the layout walk are MS3
  and are **not reshaped** — MS4 attaches **alongside** `ScalarDiscovery` (the seam MS3
  pinned: per-basin gridded subtree paths on `LayoutModel`/`BasinDir`, the `outlines`
  presence fact on `RootRollupPresence`).
- **Inert / agnostic discipline (spec §1/§11).** No type or field added by MS4 carries
  transform, role, semantic type, or provenance. The six-field `Manifest` is untouched.
  `GridInfo` carries only structural facts (grid label, extent/affine/resolution, CRS).
  Gridded fields are ordinary `Field`s (exactly `name`/`quadrant`/`dtype`/`units`/
  `grid_label`) — names taken **verbatim**, no `{source}_{variable}` / `_was_filled`
  special-casing. `delineation` labels are opaque `DelineationLabel`s.
- **No later-milestone work.** No `describe` (MS5), no `validate`/§14 rule engine (MS6),
  no CLI (MS7), no PyO3 (MS9), no describe/validate JSON schema. **No regrid / clip /
  reduce / reduction / hydrology operation, ever** (spec §10) — MS4 reads metadata only.

---

## Folded STEP-2 critique issues (each lands in a step's deliverables + test plan)

- **MED-4 — COG band-description decision (the highest-uncertainty R1 dependency).**
  S3 records, in the architecture **Amendments log**, an EXPLICIT three-outcome decision
  with the chosen outcome named: **(1)** pure-Rust read works → G1 COG-side is
  metadata-deep and live; **(2)** pure-Rust fails and GDAL accepted → record the GDAL
  system-dependency cost AND confirm MS9 maturin/PyO3 still builds; **(3)** pure-Rust
  fails and GDAL rejected → G1 COG band-name verification is an R3 byte/format-deep
  **SKIP-with-reason**, never silently claimed. The verified outcome is **(1)** (the
  `tiff 0.11` spike above). S3 **round-trips on the fixture**: a test reads back exactly
  the band descriptions the MS2 generator wrote (`elevation`, units `m`). **Never
  silently reintroduce GDAL.** If a needed band metadata read had been unreachable, the
  fix would be an **MS2 regenerate** (write the descriptions in a tag the reader
  supports), not a reader workaround — but no regenerate is needed (outcome (1)).
- **MED-5 — Zarr consolidated-metadata, Rust-side confirmation.** S2 confirms **from the
  Rust side** that the MS2 valid fixture's Zarr v3 store exposes its metadata via the §8
  **consolidated-metadata** path (one read of the root `zarr.json` learns the whole
  store). The reader reads it via the consolidated path (**live**), confirmed by a
  dedicated test. (If it could not, consolidated-metadata / v3-sharding verification
  would be classified an **R3 byte-deep SKIP with a stated reason** — documented, never
  silently claimed; this fallback is named in the S2 docs as the contingency, but
  outcome is live.) A zarr-python vs Rust mismatch is fixed by **regenerating the
  fixture**, never a reader workaround.
- **LOW-3 — no-gridded-chunk / no-pixel review gate (mandatory tests).** S2 asserts (a
  test) that the Zarr reader returns the full `GridInfo` + gridded catalog **from
  metadata + the 1-D coordinate arrays only**, decoding **no** data-variable `c/` chunk
  (verified by a test that removes/ignores the data `c/` payloads and still gets the
  full result, and by the reader only ever opening `zarr.json` + the 1-D coord chunks).
  S3 asserts (a test) the COG reader reads **tags only** and never seeks a strip/tile
  pixel offset. Both tests are **mandatory** (the central architecture §1 guarantee),
  not hedged. The softer "where feasible" phrasing is reserved only for S5's layer-level
  roll-up note.

---

## How the prior (iteration-1) critique is addressed

- **HIGH (MS4-S2 not-green: int8 mask vs closed `Dtype`).** Fixed at the root: **S1 adds
  `Dtype::I8`** (enum arm + `parse_dtype` `"i8" | "int8"` + `as_str` `"i8"` + doc-table
  row + round-trip tests + the architecture Dtype-churn amendment note). S2's catalog
  then reads the int8 mask as `Dtype::I8` (asserted), so S2 is green as written. The
  contradictory "typed-error-first reject" default is removed for this known case;
  typed-error-first is kept only for genuinely unmapped Zarr dtypes (e.g. `uint8`,
  `complex128`).
- **MED (S1 ordering): put the int8 decision in S1.** Done — `Dtype::I8` is an S1 change,
  exercised by S1 unit tests, keeping S1 green and making S2 a mechanical application.
- **MED (S2 vague CRS acceptance): pin which CRS string is recorded.** S2 acceptance
  records the `Crs` **verbatim from the `spatial_ref` attribute (`"EPSG:4326"`)** and
  asserts `crs_wkt` is **not** parsed/normalized; S3 records the COG CRS verbatim from the
  georef tags. Stated for both so S5's observation and MS6's M5 compare like with like.
- **MED (S1 convention): decide geoparquet error reuse-vs-new + the hardcoded count.** S1
  adds a **distinct `GeoparquetRead`** variant (decided, not deferred) plus `ZarrRead`,
  `CogRead`, `MissingOutlinesColumn`, and **updates `assert_eq!(variants.len(), 15)` →
  `19`** with a constructor for each new variant; `#[non_exhaustive]` retained, no MS3
  variant reshaped.
- **MED (S2 missing coverage): crs/lat/lon/time are NOT fields.** S2 acceptance states the
  catalog is **exactly** `{era5_precipitation, era5_precipitation_was_filled}` (the two
  `["time","lat","lon"]` members); `crs`/`lat`/`lon`/`time` are read for georef/extent only
  and **never** catalogued nor passed to the dtype bridge — asserted in a test (so the
  scalar `crs` int32 array never trips `Field::new`'s `MismatchedGridLabel`).
- **LOW (S2 README dtype window): update the glossary dtype line.** S1 updates the
  `parse_dtype` doc-table; **S5 updates the README glossary dtype list to include `i8`**
  (stated explicitly in S5). The README is not compiled, so the only "inaccuracy window"
  is documentation, closed at S5.
- **LOW (S3 vague-acceptance: drop "where feasible" for the two highest-risk readers).**
  The S2 no-data-chunk test and the S3 no-pixel test are **mandatory**; "where feasible"
  is reserved only for S5's layer-level roll-up.
- **LOW (S5 untestable mermaid claim).** Reworded: the README module map listing the four
  new nodes with edges matching `lib.rs`'s `//!` is a **review obligation, not a test** —
  no test is claimed for Markdown/Mermaid.
- **LOW (S2/S3 dtype bridge not exhaustive).** Both bridges are pinned as **closed**
  mappings (Zarr: `int8→I8`, `float32→F32`, `float64→F64`, `int32→I32`, `int64→I64`,
  `bool→Bool`; COG: IEEE-float-32→F32, IEEE-float-64→F64, signed-int-32→I32,
  signed-int-64→I64, signed-int-8→I8), and each step asserts the unmapped case →
  `UnknownDtype`; the `?` on int8 is removed (`Dtype::I8` exists from S1).

---

## Ordering rationale

MS4 is built bottom-up: each format reader is its own committable unit that compiles,
tests against the real MS2 fixture, and leaves the repo green; the combined-discovery
seam lands last once all three readers exist. The order is dependency-sequential:

1. **S1 — Shared gridded model + error surface + `Dtype::I8` (types first, no IO).**
   Freeze the type shapes every reader produces (`GridInfo`, `GriddedDiscovery` shell,
   `GeometryDiscovery` shell), add the new `thiserror` variants (`ZarrRead`, `CogRead`,
   `MissingOutlinesColumn`, `GeoparquetRead`), and — critically — **extend the closed
   `Dtype` enum with `I8`** so S2's int8 mask catalogs without error. Doing types first
   removes the prior HIGH issue's ambiguity: S2 is then a mechanical application of an
   already-present `Dtype::I8`. S1 is pure types + unit tests, trivially green.
2. **S2 — Zarr v3 metadata reader (gridded·dynamic).** The hardest reader and the
   MED-5/LOW-3 gates. Reads the consolidated `zarr.json` → gridded·dynamic `Field`s
   (dtype via the S1-extended bridge incl. `int8→I8`), the 1-D `lat`/`lon`/`time`
   coordinate arrays → extent/affine/resolution, the CF `grid_mapping`/`crs` member →
   verbatim CRS, CF `units`. It depends only on S1's types.
3. **S3 — COG metadata reader (gridded·static).** The MED-4 gate. Reads the GeoTIFF tags
   (band description from tag 42112 GDAL_METADATA XML → field name; SampleFormat+
   BitsPerSample → dtype; georef tags → extent/affine/resolution + verbatim CRS) via
   pure-Rust `tiff 0.11`. Depends only on S1's types. Independent of S2.
4. **S4 — Geoparquet metadata reader (geometry).** Reads `outlines.geoparquet` schema →
   the `(basin_id, delineation, geometry)` structural check + the `delineation` /
   `basin_id` 1-D column reads, reusing MS3's `parquet`/`arrow` path. Depends only on
   S1's types (the `MissingOutlinesColumn` / `GeoparquetRead` variants).
5. **S5 — Combined discovery seam + README/module-map.** Ties S2/S3/S4 to the layout
   walk and `ScalarDiscovery` into one `discover_gridded` (+ a combined model the verbs
   consume), running against the real fixture per basin. Updates `crates/core/README.md`
   (module map + glossary, incl. the `i8` dtype line). Lands last because it consumes all
   three readers.

Readers (S2–S4) are mutually independent given S1, so they could be reordered among
themselves; the sequence S2→S3→S4 puts the highest-risk reader (Zarr/MED-5) first so
its surprises surface early, then the second-highest (COG/MED-4), then the lowest-risk
(geoparquet, reusing MS3's proven path).

---

## Step MS4-S1 — Shared gridded/geometry model + error surface + `Dtype::I8`

**Intent.** Stand up — types only, zero IO — the in-memory shapes every MS4 reader will
produce, the new typed error variants, and the **closed-enum `Dtype::I8` extension** the
int8 mask requires. Freezing the shapes here means S2–S4 are mechanical applications and
cannot drift; pre-committing `Dtype::I8` removes the prior HIGH issue (int8 mask) at its
root. Independently committable and trivially green (pure types + unit tests).

**Changes.**
- `crates/core/src/field.rs`:
  - Add `Dtype::I8` (`/// 8-bit signed integer (e.g. a compact QC / gap mask, spec §12).`).
  - `parse_dtype`: add an arm `"i8" | "int8" => Dtype::I8` (the **only** new accepted
    strings; everything else still errors with `UnknownDtype`).
  - `Dtype::as_str`: add `Dtype::I8 => "i8"`.
  - Extend the documented dtype table in `parse_dtype`'s doc comment with the `i8`/`int8`
    row.
- `crates/core/src/error.rs` — add four named-field `thiserror` variants, each
  doc-commented with WHEN it fires, all **inert/agnostic** (artifact name + opaque
  detail only; no domain/provenance field), under the existing `#[non_exhaustive]`:
  - `ZarrRead { artifact: String, detail: String }` — a Zarr store's `zarr.json` /
    consolidated metadata cannot be read or fails to decode.
  - `CogRead { artifact: String, detail: String }` — a COG's tags / band metadata cannot
    be read or fails to decode.
  - `GeoparquetRead { artifact: String, detail: String }` — distinct from `ParquetRead`
    so a geoparquet failure is attributable to the geometry reader (decided here: a
    **distinct** variant, not `ParquetRead` reuse).
  - `MissingOutlinesColumn { artifact: String, column: String }` — `outlines.geoparquet`
    lacks a structurally required column (`basin_id`, `delineation`, or `geometry`).
- `crates/core/src/grid.rs` (**new module**, `pub mod grid;` in `lib.rs`): the gridded
  model types, all **inert/agnostic** (structural facts only), fields private + getters:
  - `GridExtent { west, north, x_res, y_res, width, height }` (or equivalent) — the
    dense rectangular extent/affine/resolution of one grid (spec §7.1); a distinct type
    so it cannot be confused with a coordinate at a call site.
  - `GridInfo { grid_label: GridLabel, extent: GridExtent, crs: Crs }` — per-grid-label
    extent/affine/res/CRS (architecture §3.5 `grids`); CRS recorded **verbatim**.
  - A `GriddedDiscovery` shell (`gridded_fields: Vec<Field>`, `grids: Vec<GridInfo>`) and
    a `GeometryDiscovery` shell (`delineations: Vec<DelineationLabel>`,
    `has_basin_id: bool`, schema-presence flags) with getters — populated in S2–S5;
    declared here so the shapes are frozen.
- `crates/core/src/lib.rs` — add `pub mod grid;`; **bump** the
  `every_core_error_variant_constructs` test: construct the four new variants AND update
  the `assert_eq!(variants.len(), 15)` literal to **`19`**.

**Test plan.**
- `field.rs`: extend `parse_dtype_maps_every_documented_string` with `i8`/`int8` → `I8`;
  extend `parse_dtype_round_trips_canonical_strings` to include `Dtype::I8` (and its
  `as_str` == `"i8"`); keep `parse_dtype_rejects_unknown_without_panicking` and add
  `u8`/`int16`/`uint8` as still-`UnknownDtype` cases.
- `lib.rs`: `every_core_error_variant_constructs` constructs `ZarrRead`, `CogRead`,
  `GeoparquetRead`, `MissingOutlinesColumn`; the count literal is `19`; every variant
  renders a non-empty `Display`.
- `grid.rs`: a unit test constructing a `GridInfo` and round-tripping its getters
  (extent components, `grid_label`, `crs` verbatim); a test that the discovery shells
  expose their accessors (compile/shape check) and carry no derived field.

**Acceptance.**
- `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings` all pass.
- `parse_dtype("int8")` and `parse_dtype("i8")` return `Dtype::I8`; `Dtype::I8.as_str()
  == "i8"` and re-parses; no other new dtype string is accepted (typed-error-first for
  genuinely unmapped dtypes preserved: `u8`/`uint8`/`int16` → `UnknownDtype`).
- The error surface is exactly the four new variants plus MS1–MS3's existing ones; the
  hardcoded count literal in `every_core_error_variant_constructs` is updated to `19` and
  the test passes; `#[non_exhaustive]` retained; no MS1–MS3 variant reshaped.
- `GridInfo`/`GriddedDiscovery`/`GeometryDiscovery` exist with private fields + getters,
  carry no transform/role/semantic/provenance field, and CRS is stored verbatim.
- Spec-check ids advanced (type scaffolding; enforced MS6): **H1** (dtype set now
  includes the conformant `int8` mask so the homogeneous schema is fully representable),
  **G1/G3** (GridInfo shape for self-naming + georef facts).

**Spec refs.** §1 (inert/agnostic), §2 (field model; ordinary fields), §7 (per-variable
native grids; one dataset-wide CRS), §8 (self-naming), §12 (a mask is an ordinary
int/bool field — int8 is a legitimate mask encoding); architecture §3.3/§3.5, §7 R1;
the MS1 Dtype-churn policy (the closed-enum addition is recorded as an architecture
amendment — landed with the S2/S3 reader amendments, noted in S1's commit).

**Commit message.** `feat(core): add Dtype::I8 and the MS4 gridded/geometry model + error variants`

---

## Step MS4-S2 — Zarr v3 metadata reader (gridded·dynamic; MED-5 + LOW-3 gates)

**Intent.** Read a basin's `gridded_dynamic/<label>.zarr` **metadata only** into the
gridded·dynamic `Field` catalog + a `GridInfo`, via the §8 **consolidated-metadata**
path. This is the MED-5 Rust-side confirmation and the LOW-3 no-data-chunk gate. The int8
mask now catalogs cleanly (S1's `Dtype::I8`). Independently committable and green against
the real fixture.

**Changes.**
- `crates/core/Cargo.toml` — **no `zarrs` dependency.** The consolidated `zarr.json` is
  plain JSON; reuse the existing `serde`/`serde_json`. The 1-D coordinate arrays
  (`lat`/`lon`/`time`) are `bytes`+`zstd` single-chunk; decode with a **pure-Rust zstd
  decoder** (the `zstd` crate already in the tree via `parquet`'s `zstd` feature, or a
  minimal `ruzstd` if the codec is not directly callable — decide at implementation;
  **pure-Rust only, no GDAL/C deps**). Record the R1-Zarr decision in the architecture
  Amendments log: **pure-Rust `serde_json` over the consolidated `zarr.json` + a pure-Rust
  zstd decode of the 1-D coordinate chunks; `zarrs` not needed for metadata-only
  discovery** (and the closed-enum `Dtype::I8` addition from S1 is noted in the same
  amendment per the MS1 Dtype-churn policy).
- `crates/core/src/zarr_reader.rs` (**new module**, `pub mod zarr_reader;`):
  - `read_zarr_store(path) -> Result<ZarrStore, CoreError>`: open `<store>/zarr.json`,
    parse the inline `consolidated_metadata.metadata` map (the **§8 consolidated path**).
    If `consolidated_metadata` is absent, fall back to reading each member's `zarr.json`
    AND record that the consolidated path was unavailable (the R3 skip-with-reason
    contingency) — but the fixture has it, so the live path is the consolidated one
    (asserted by a test).
  - Enumerate **data arrays** = members whose `dimension_names == ["time","lat","lon"]`
    (i.e. `era5_precipitation`, `era5_precipitation_was_filled`); each → one
    `Quadrant::GriddedDynamic` `Field` with: name = member name **verbatim**, dtype via a
    **closed** Zarr-dtype bridge (`float32→F32`, `float64→F64`, `int32→I32`, `int64→I64`,
    `int8→I8`, `bool→Bool`; **any other Zarr `data_type` → `UnknownDtype`**), units from
    the CF `units` attribute (or `Units::none`), `grid_label` = the store's `<label>`
    (from the `.zarr` filename).
  - **The `crs`/`lat`/`lon`/`time` members are read for georef/extent ONLY** — they have
    no `["time","lat","lon"]` dimension_names, so they are **never catalogued as Fields**
    nor passed through the dtype bridge (so the scalar `crs` int32 array never reaches
    `Field::new` and never trips `MismatchedGridLabel`).
  - `GridInfo` assembly: read the **1-D `lat` and `lon` coordinate arrays** (the
    architecture-§1-sanctioned coord read — a single small `bytes`+`zstd` chunk each) to
    derive west/north/x_res/y_res/width/height; CRS recorded **verbatim from the `crs`
    member's `spatial_ref` attribute** (`"EPSG:4326"`); `crs_wkt` is **not** parsed.
  - CF georef presence facts (G3): `lat`/`lon` coordinate arrays present + each data
    array's `grid_mapping` attribute present (recorded, not enforced).
  - All Zarr read/parse failures → `CoreError::ZarrRead { artifact, detail }`.
- `crates/core/src/lib.rs` — add `pub mod zarr_reader;` and a module-map `//!` line.

**Test plan (against `conformance/valid/minimal/basin=0001/gridded_dynamic/era5.zarr`).**
- **Catalog (exactly two, HIGH issue resolved).** The gridded·dynamic catalog is
  **exactly** `{era5_precipitation: GriddedDynamic/F32/units "mm",
  era5_precipitation_was_filled: GriddedDynamic/I8/units "1"}`, both with
  `grid_label == "era5"`. Assert the int8 mask is read as **`Dtype::I8`** (not
  `UnknownDtype`). Assert the catalog is EXACTLY those two — `crs`/`lat`/`lon`/`time` are
  NOT catalogued as fields and are NOT passed to the dtype bridge.
- **Ordinary fields, no name magic.** `era5_precipitation` (`{source}_{variable}`) and
  `era5_precipitation_was_filled` (`_was_filled`) are ordinary `Field`s — names verbatim,
  no suffix/prefix special-casing, no belongs-to (spec §2).
- **MED-5 consolidated path (live).** A dedicated test asserts the reader reads the
  store's metadata via the **inline `consolidated_metadata`** in the root `zarr.json` (one
  read learns the store): assert the parsed member set comes from the consolidated map,
  and that the full catalog + `GridInfo` are produced **without** opening any member's own
  `zarr.json`. (If consolidated metadata were absent, the test would assert the R3
  skip-with-reason path — but on the fixture it is live.) Comment: a zarr-python vs Rust
  mismatch is fixed by **regenerating the fixture**, never a reader workaround.
- **LOW-3 no-data-chunk (MANDATORY).** A test proves the reader **never decodes a gridded
  data array's `c/` chunk**: copy the store to a temp dir, **delete** the
  `era5_precipitation/c/**` and `era5_precipitation_was_filled/c/**` chunk payloads, and
  assert the reader **still returns the full `GridInfo` + the two-field catalog** (it read
  only `zarr.json` + the 1-D `lat`/`lon` coordinate chunks). This makes the "data chunks
  are opaque leaves" guarantee a hard, tested fact.
- **CRS verbatim.** Assert `GridInfo.crs() == Crs::new("EPSG:4326")` sourced from
  `spatial_ref`; assert `crs_wkt` is not parsed (the recorded CRS is the short EPSG
  string, not a WKT).
- **GridInfo extent.** Assert width=6, height=8, x_res=0.25, y_res=0.25, west=10.0,
  north=50.0 (the generator's known geometry), derived from the 1-D `lat`/`lon` arrays.
- **Dtype bridge exhaustive + unmapped errors.** Unit tests pin the closed mapping
  (`float32→F32`, `float64→F64`, `int32→I32`, `int64→I64`, `int8→I8`, `bool→Bool`) and
  assert an unmapped Zarr `data_type` (e.g. `"complex128"`, `"uint8"`) → `UnknownDtype`.
- **Negative.** A non-existent / malformed store → `ZarrRead`.

**Acceptance.**
- `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings` all pass.
- The Zarr reader catalogs **both** data variables incl. the int8 mask as `Dtype::I8`;
  the catalog is exactly the two `["time","lat","lon"]` members; `crs`/`lat`/`lon`/`time`
  are read for georef/extent only and never catalogued.
- **MED-5 live:** a test confirms metadata is read via the §8 consolidated path (one
  `zarr.json` read learns the store). No `zarrs` / GDAL dependency added; the R1-Zarr
  decision is recorded as an architecture amendment.
- **LOW-3:** the mandatory no-data-chunk test passes — the reader produces the full
  result with the data `c/` payloads removed.
- CRS recorded verbatim from `spatial_ref`; extent derived from the 1-D coord arrays;
  the dtype bridge is the exact closed set and unmapped → `UnknownDtype`.
- Spec-check ids advanced (discovery foundations; enforced MS6): **G1** (self-naming CF
  variables = field names; no positional channel axis), **G3** (CF `lat`/`lon` +
  `grid_mapping` present), **H1** (full gridded·dynamic schema incl. the int8 mask),
  **M5** input (per-file CRS read; cross-check is MS6).

**Spec refs.** §1 (inert/agnostic), §2 (ordinary fields; self-naming), §6.3 (CF
integer-since-epoch `time`), §7 (per-variable native grids; CF georef; one dataset-wide
CRS), §8 (one artifact = one grid; consolidated metadata; v3 sharding present-but-not-
decoded), §12 (int8 mask is an ordinary field); architecture §1 (metadata + 1-D coords,
no chunks), §3.5, §7 R1/R3.

**Commit message.** `feat(core): add Zarr v3 metadata reader via the consolidated path`

---

## Step MS4-S3 — COG metadata reader (gridded·static; MED-4 gate)

**Intent.** Read a basin's `gridded_static/<label>.tif` **tags + band metadata only**
into the gridded·static `Field` catalog + a `GridInfo`, via pure-Rust `tiff 0.11`. This
is the MED-4 decision + round-trip and the COG-side LOW-3 no-pixel gate. Independently
committable and green against the real fixture.

**Changes.**
- `crates/core/Cargo.toml` — add **`tiff = "0.11"`** (pure-Rust, **no GDAL/C deps**).
  Comment: pinned to 0.11 because 0.9's `find_tag`/`get_tag(Value)` returns `None` for
  unknown geo tags; 0.11's typed `get_tag_*_vec` accessors read tag 42112 + the geo tags
  reliably (verified by spike).
- `crates/core/src/cog_reader.rs` (**new module**, `pub mod cog_reader;`):
  - `read_cog(path) -> Result<CogInfo, CoreError>`: open with `tiff::decoder::Decoder`;
    read **tags only** (never a strip/tile pixel offset):
    - **Band description → field name (G1).** Read tag **42112** via
      `get_tag_ascii_string(Tag::Unknown(42112))`, parse the GDALMetadata XML, and extract
      the `<Item ... role="description">…</Item>` per band as the field name; extract the
      matching `<Item name="units" sample="…">` as `Units`. (Tag 270 is absent in the
      fixture; the spec/fixture put the band description in 42112, so 42112 is the primary
      read.) The single-band COG → one `Quadrant::GriddedStatic` `Field` named `elevation`,
      units `m`.
    - **Dtype bridge (closed).** `SampleFormat` (tag 339) + `BitsPerSample` (tag 258):
      IEEE-float + 32 → `F32`; IEEE-float + 64 → `F64`; signed-int + 32 → `I32`;
      signed-int + 64 → `I64`; signed-int + 8 → `I8`; **any other combination →
      `UnknownDtype`**. `grid_label` = the `.tif` filename label.
    - **Georef → `GridInfo` (G3).** Read `ModelPixelScale` (33550) + `ModelTiePoint`
      (33922) → west/north/x_res/y_res; `ImageWidth`/`ImageLength` (256/257, or
      `Decoder::dimensions`) → width/height; CRS recorded **verbatim** from the georef
      (resolve EPSG via `GeoKeyDirectory`(34735) / `GeoAsciiParams`(34737) →
      `Crs::new("EPSG:4326")`); **do not** synthesize/normalize a WKT.
  - All `tiff` errors mapped to `CoreError::CogRead { artifact, detail }`.
- `crates/core/src/lib.rs` — add `pub mod cog_reader;` and a module-map `//!` line.
- `architecture.md` Amendments log — **MED-4 three-outcome decision recorded** with the
  chosen outcome named: **outcome (1)** (pure-Rust `tiff 0.11` reads the band description
  in tag 42112 + georef + SampleFormat/BitsPerSample; **no GDAL**). The two unchosen
  outcomes are stated for the record: (2) GDAL-accepted would require recording the
  system-dependency cost + confirming the MS9 maturin/PyO3 wheel still builds with it;
  (3) GDAL-rejected would make G1 COG band-name verification an R3 byte/format-deep
  SKIP-with-reason. **Never silently reintroduce GDAL.** Note the `tiff 0.9` `find_tag`
  caveat and the 0.11 typed-accessor requirement.

**Test plan (against `conformance/valid/minimal/basin=0001/gridded_static/era5.tif`).**
- **MED-4 round-trip (live).** A test reads back **exactly** the band description the MS2
  generator wrote: the single band → `Field` named `elevation`, `Quadrant::GriddedStatic`,
  `Dtype::F32`, units `Some("m")`, `grid_label == "era5"`. This proves the chosen reader
  reads the descriptions the generator wrote on the actual fixture (no MS2 regenerate
  needed).
- **Ordinary field, no name magic.** `elevation` is an ordinary `Field` (name verbatim,
  no special handling).
- **Georef → GridInfo (G3).** Assert width=6, height=8, x_res=0.25, y_res=0.25, west=10.0,
  north=50.0 from `ModelPixelScale`/`ModelTiePoint`/dimensions; assert
  `GridInfo.crs() == Crs::new("EPSG:4326")` resolved verbatim from the georef.
- **Dtype bridge.** Pin SampleFormat=3 + BitsPerSample=32 → `F32`; a unit test asserts an
  unmapped (SampleFormat, BitsPerSample) combination → `UnknownDtype`.
- **LOW-3 no-pixel (MANDATORY).** A test proves the reader reads **tags only** and never a
  pixel raster: the reader uses only `Decoder` tag accessors (`get_tag_*` /
  `dimensions`) and **never** calls `read_image`/strip/tile decode — verified by the call
  graph and asserted by a test that the full result is produced with no image decode (the
  strip/tile pixel bytes are never touched).
- **Negative.** A non-existent / non-TIFF file → `CogRead`.

**Acceptance.**
- `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings` all pass.
- The COG reader catalogs the single `elevation` band as `GriddedStatic`/`F32`/units `m`,
  `grid_label == "era5"` — the exact bytes the generator wrote (MED-4 round-trip).
- **MED-4 outcome (1) recorded** in the architecture Amendments log with all three named
  outcomes; **no GDAL** dependency; `tiff = "0.11"` pinned with the typed-accessor note.
- CRS recorded verbatim from the georef tags; extent derived from georef tags;
  unmapped dtype → `UnknownDtype`.
- **LOW-3 COG-side:** the mandatory no-pixel test passes — the reader reads tags only.
- Spec-check ids advanced (discovery foundations; enforced MS6): **G1** (COG band
  description = field name; no positional channel axis), **G3** (standard GeoTIFF georef
  present), **G2/H2** input (the COG's `grid_label` = `era5`, the same label as the Zarr —
  shared-label observation completed in S5), **M5** input (per-file CRS read).

**Spec refs.** §1 (inert/agnostic), §2 (ordinary fields; self-naming band = field name),
§7 (per-variable native grids; standard GeoTIFF georef; one dataset-wide CRS), §8
(gridded static → multiband COG; band description = field name; internal tiling/overviews
present-but-not-decoded); architecture §1 (tags only, no pixels), §3.5, §7 R1/R3.

**Commit message.** `feat(core): add COG metadata reader (band desc + georef via tiff)`

---

## Step MS4-S4 — Geoparquet metadata reader (geometry; Geo1 + I1 inputs)

**Intent.** Read the root `outlines.geoparquet` **metadata only** into the geometry
discovery facts — the `(basin_id, delineation, geometry)` structural schema check, the
`delineation` labels, and `basin_id` presence — reusing MS3's proven `parquet`/`arrow`
metadata + bounded 1-D column path (no new geometry crate). Independently committable and
green against the real fixture.

**Changes.**
- `crates/core/Cargo.toml` — **no new dependency**; reuse the existing `parquet`/`arrow`
  stack. (geoarrow.wkb geometry is read structurally as an arrow `binary` column; MS4
  does **not** decode geometries — full geometries are out of scope, architecture §1.)
- `crates/core/src/outlines_reader.rs` (**new module**, `pub mod outlines_reader;`):
  - `read_outlines(path) -> Result<OutlinesInfo, CoreError>`: open the geoparquet footer
    (metadata only); confirm the schema **structurally carries** the three required
    columns `basin_id`, `delineation`, `geometry` (a structurally required column that is
    absent → `MissingOutlinesColumn`, since the reader cannot model outlines without it;
    the conformance verdict Geo1 is still MS6).
  - Read the **`delineation` column** (a bounded 1-D `delineation`-only read, reusing
    MS3's `ProjectionMask::leaves` pattern) → the **distinct** `DelineationLabel`s in
    first-seen order (`merit`, `grit`).
  - Read **`basin_id` presence** (I1 input) and the distinct `basin_id` values (a bounded
    1-D read, same as MS3's scalar `basin_id` read).
  - Structural "not partitioned by delineation" fact: `outlines.geoparquet` is a **single
    file at the root** (the layout walk already records it as a root rollup) — recorded as
    a fact; the verdict is MS6.
  - geoparquet read/decode failures → `CoreError::GeoparquetRead`; a missing required
    column → `CoreError::MissingOutlinesColumn`.
- `crates/core/src/lib.rs` — add `pub mod outlines_reader;` and a module-map `//!` line.

**Test plan (against `conformance/valid/minimal/outlines.geoparquet`).**
- **Schema (Geo1 input).** Assert the schema carries exactly the three columns
  `basin_id` (utf8/string), `delineation` (utf8/string), `geometry` (binary); assert the
  label column is present and named `delineation`.
- **Delineation labels.** Assert the distinct labels are `[merit, grit]` (plural; spec §9)
  in first-seen order.
- **`basin_id` (I1 input).** Assert `basin_id` is present; the distinct in-file values
  include `0001/0002/0003` (recorded, not cross-checked — that is MS6's I1).
- **Not-partitioned fact.** Assert the reader treats `outlines.geoparquet` as a single
  root file (no `delineation=<x>/` partition subtree), recorded as a fact.
- **No geometry decode.** Assert the reader reads the schema + the `delineation`/`basin_id`
  columns only — the `geometry` (WKB binary) column is **never decoded** (architecture §1:
  full geometries are out of scope).
- **Negative.** A geoparquet missing the `delineation` column → `MissingOutlinesColumn`
  (synthesize an in-test parquet without it, like MS3's in-test parquet helpers); a
  non-parquet file → `GeoparquetRead`.

**Acceptance.**
- `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings` all pass.
- The geometry reader surfaces the `(basin_id, delineation, geometry)` schema fact, the
  `delineation` labels `[merit, grit]`, and `basin_id` presence — all from the real
  fixture; the `geometry` WKB column is never decoded.
- A missing required column → `MissingOutlinesColumn`; a malformed file → `GeoparquetRead`;
  **no new dependency** added (reuses MS3's `parquet`/`arrow`).
- Spec-check ids advanced (discovery foundations; enforced MS6): **Geo1** (outlines schema
  `(basin_id, delineation, geometry)`; label column `delineation`; not partitioned), **I1**
  (`basin_id` in outlines).

**Spec refs.** §1 (inert/agnostic; never decode geometries), §3 (`basin_id` in outlines),
§9 (plural outlines; one row per delineation; neutral `delineation` label; one root file,
not partitioned), §14 Geo1, I1; architecture §1 (schema + 1-D columns, no geometry decode),
§3.5, §7 R1.

**Commit message.** `feat(core): add geoparquet outlines metadata reader`

---

## Step MS4-S5 — Combined discovery seam + crate README/module map

**Intent.** Tie S2/S3/S4 to MS3's layout walk + `ScalarDiscovery` into one boundary
function that completes the shared discovery layer, running per basin against the real
fixture, and update `crates/core/README.md`. Lands last because it consumes all three
readers. Independently committable and green.

**Changes.**
- `crates/core/src/discovery.rs` (extend; **do not reshape** `ScalarDiscovery`):
  - Add `discover_gridded(path) -> Result<GriddedGeometryDiscovery, CoreError>` (name per
    implementation) that walks the layout (or accepts the `LayoutModel`), and for each
    basin's `gridded_static/<label>.tif` and `gridded_dynamic/<label>.zarr` calls S3/S2,
    and reads the root `outlines.geoparquet` via S4. It assembles: the gridded field
    catalog (one representative basin, §5 one-basin discovery), the `Vec<GridInfo>` per
    grid label, the `delineation` labels, and the **shared-grid-label observation** (the
    COG label and the Zarr label per basin, recorded side by side — **observed, never
    asserted-equal**; G2 precondition for MS6).
  - Provide a combined accessor / umbrella that holds `ScalarDiscovery` **alongside** the
    gridded/geometry facts (the seam MS3 pinned), so MS5/MS6 consume one model. **Records
    facts, enforces nothing** (gaps surfaced; structural failures propagate as the typed
    `CoreError`).
- `crates/core/README.md`: update the **Mermaid module map** to add the four new nodes
  (`grid`, `zarr_reader`, `cog_reader`, `outlines_reader`) and their edges to
  `discovery`/`error`/`field`/`newtypes`, matching the real use-graph in `lib.rs`'s
  module-map `//!`; extend the **glossary** with: gridded·dynamic / gridded·static, grid
  label, consolidated metadata, CF georef, band description, delineation, and the **dtype
  list updated to include `i8`** (this is the explicit place the README glossary dtype
  line is brought current with `Dtype::I8` from S1).
- `crates/core/src/lib.rs` — update the module-map `//!` to describe the four new modules
  + the combined-discovery seam.

**Test plan (against `conformance/valid/minimal/`).**
- **Per-basin gridded read.** For each of `basin=000{1,2,3}`, assert the gridded·dynamic
  catalog is the two ordinary fields (`era5_precipitation`/F32, `era5_precipitation_was_filled`/I8),
  the gridded·static catalog is the one `elevation`/F32 field, and a `GridInfo` per grid
  label is produced.
- **Shared-grid-label observation (G2 precondition).** Assert that, per basin, the COG's
  `grid_label` and the Zarr's `grid_label` are both observed as `era5` and **recorded side
  by side** — the test documents they coincide on the conformant fixture but the seam does
  **not** enforce equality (that is MS6's G2).
- **Delineations.** Assert the combined model surfaces `delineation` labels `[merit, grit]`.
- **Combined model shape.** Assert the umbrella/seam exposes both `ScalarDiscovery` (from
  MS3, unreshaped) and the gridded/geometry facts — a compile + shape check pinning the
  accessors MS5/MS6 will consume.
- **Ordinary-field discipline (roll-up — softer "where feasible" per LOW-3).** Assert
  across the whole combined model no field carries a grid label it shouldn't, no
  name-pattern special-casing occurred, and the model holds only structural facts (no
  transform/role/semantic/provenance surface to read).

**Acceptance.**
- `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings` all pass.
- The combined discovery layer is complete: both verbs (MS5/MS6) can assemble from one
  typed in-memory model; `ScalarDiscovery` is **unreshaped** (MS3's accessors intact).
- The shared grid label is **observed** across the static/dynamic subtrees (G2
  precondition), never asserted-equal in MS4.
- `crates/core/README.md`'s Mermaid module map lists the four new nodes
  (`grid`, `zarr_reader`, `cog_reader`, `outlines_reader`) with edges matching the real
  use-graph in `lib.rs`'s `//!` module map — **verified by review, not by a test** (cargo
  does not compile Markdown/Mermaid and no mermaid linter is wired in, so no test is
  claimed for the README). The glossary dtype list includes `i8`.
- Spec-check ids advanced (discovery foundations complete; enforced MS6): **G1, G2
  (precondition observed), G3, Geo1, I1, H2 (grid-label set per basin), M5 (per-file CRS
  read)** — the gridded/geometry half of the §14 fact set is now discoverable.

**Spec refs.** §1 (inert/agnostic), §2 (ordinary fields), §5 (one-basin discovery), §7
(per-grid info; CF/GeoTIFF georef), §8 (shared grid label ⇒ alignment — observed), §9
(delineation labels), §14 G1–G3, Geo1, I1, H2; architecture §1, §3.5, §5, §7 R1/R3.

**Commit message.** `feat(core): complete the discovery layer with the gridded/geometry seam`

---

## Per-step commit discipline (CLAUDE.md / AGENTS.md — non-negotiable)

Every step is exactly **one** conventional commit and leaves the repo green. Before each
commit: `./scripts/bump-version.sh patch`, stage `Cargo.toml` alongside the code, commit
with the step's `commit_message`, then `git tag v<version>`. Never let tooling create its
own commit/tag; fold the version bump into the real commit. `tracing` only (no `println!`
for diagnostics); `#[instrument]` on public reader fns (skip large args). Library code
uses `thiserror` named-field variants and never `unwrap`/`expect`/`panic`. Imports are
explicit, grouped std → external → crate-internal. Edition 2024.

## Coverage map — every MS4 deliverable / exit criterion / spec ref is assigned

| MS4 deliverable / exit criterion (milestones.md) | Step(s) |
|---|---|
| R1 decision for gridded + geometry recorded (architecture amendment) | S2 (Zarr: pure-Rust serde_json + zstd), S3 (COG: pure-Rust `tiff 0.11`), S4 (geoparquet: reuse `parquet`/`arrow`) |
| Zarr reader: per-grid-label metadata; CF vars = field names → gridded·dynamic fields; lat/lon/time coords; CF grid_mapping/CRS; units; extent/affine/res | S2 |
| §8 consolidated-metadata gate (live OR R3 skip-with-reason) | S2 (live, MED-5) |
| COG reader: band descriptions = field names → gridded·static fields; georef tags; extent/affine/res; units | S3 (MED-4) |
| Geoparquet reader: `(basin_id, delineation, geometry)` schema; `delineation`; `basin_id`; not-partitioned | S4 |
| Reader output feeds the unified discovery model (`GridInfo`, delineations, gridded catalog) alongside MS3's scalar half | S1 (types), S5 (seam) |
| Tests: Zarr vars w/ CF lat/lon + grid_mapping; COG bands w/ georef; shared grid label observed across subtrees (G2 precondition); geoparquet schema + labels; ordinary `{source}_{variable}` field | S2, S3, S4, S5 |
| Discovery layer complete (both verbs can assemble) | S5 |
| Spec-check ids advanced: G1, G2 precondition, G3, Geo1, I1 | S2 (G1/G3), S3 (G1/G3), S4 (Geo1/I1), S5 (G2 precondition observed) |
| Folded MED-4 (three-outcome decision + round-trip; never silently reintroduce GDAL) | S3 |
| Folded MED-5 (Rust-side consolidated-path confirmation; R3 fallback named) | S2 |
| Folded LOW-3 (no gridded-chunk decode / no pixel raster — mandatory tests) | S2 (no data chunk), S3 (no pixel), S5 (roll-up) |
| HIGH (prior critique): int8 mask → `Dtype::I8` (closed-enum extension, not MS2-regenerate) | S1 (enum + parse + as_str + docs), S2 (catalogs int8 mask as I8) |
| CRS read verbatim, M5 deferred to MS6 | S2 (`spatial_ref` verbatim), S3 (georef verbatim), scope guard |
| bump+tag / conventional commit per step | all steps |
