# MS4 — Gridded + geometry metadata readers (discovery layer, gridded/geometry half)

> **Milestone goal (verbatim intent).** Complete the shared discovery layer: read
> Zarr v3 metadata/attrs + 1-D `lat`/`lon`/`time` coordinate facts + CF
> georeferencing; read COG band descriptions + standard georef tags; read
> `outlines.geoparquet` schema + `delineation`/`basin_id` columns + CRS. Land the
> hard **R1** crate decision for Zarr / COG / geometry, and explicitly resolve the
> **§8 consolidated-metadata / sharding** read question. **Metadata + 1-D
> coordinate reads only — NEVER gridded chunks / pixel rasters (architecture §1).**
>
> **Source contract:** `spec/HDX_SPEC.md`. **Planned against:** `architecture.md`.
> **Milestone definition:** `planning/milestones.md` → MS4.
>
> This is **iteration 3** of the MS4 step plan. Every blocking issue from the prior
> step-critic (MED-1 unpinned zstd decoder; MED-2 time-coord over-claim; LOW-3 S1
> clippy-green precondition; LOW-4 subtree→artifact resolution) is addressed below
> and itemized in the returned `addressed_issues`.

---

## Starting state (ground-truthed against the repo + fixture bytes)

The discovery layer's **scalar half** already exists and is green
(`crates/core/src/{layout,scalar_reader,parquet_meta,discovery}.rs`). MS4 attaches
the **gridded + geometry half** *alongside* `ScalarDiscovery` without reshaping it
(the documented MS3 seam: `BasinDir::gridded_static()` / `gridded_dynamic()` paths
+ `RootRollupPresence::outlines_present()`).

Facts verified by reading the committed fixture bytes (`conformance/valid/minimal/`):

- **Zarr** `gridded_dynamic/era5.zarr` is **v3 with inline `consolidated_metadata`**
  in the store-root group `zarr.json` (the §8 one-read-to-learn-the-store path is
  physically present). Arrays: `era5_precipitation` (`float32`, sharded+blosc),
  `era5_precipitation_was_filled` (**`int8`**, sharded+blosc), `lat`/`lon`
  (`float64`, codecs `bytes`+`zstd`), `time` (`int64`, `bytes`+`zstd`,
  `units="days since 1970-01-01"`, `calendar="proleptic_gregorian"`), `crs`
  (`int32` scalar) carrying `grid_mapping_name`, `crs_wkt`, `spatial_ref="EPSG:4326"`.
  Variables carry `units`, `grid_mapping: "crs"`, `_ARRAY_DIMENSIONS`/`dimension_names`.
  The `lat/c/0`, `lon/c/0`, `time/c/0` chunk files begin with the **zstd magic
  `28 b5 2f fd`** — decoding any coordinate value requires a zstd decoder.
- **COG** `gridded_static/era5.tif` carries georef tags **33550** (ModelPixelScale
  `0.25,0.25`), **33922** (ModelTiepoint → west `10`, north `50`), **34735**
  (GeoKeyDirectory → EPSG:4326). The **band description `"elevation"` and units
  `"m"` live in the GDAL metadata XML in tag 42112 (`GDAL_METADATA`)**, NOT in
  TIFF `ImageDescription` (270). Compression is `AdobeDeflate`; SampleFormat is
  IEEE float (f32).
- **Geoparquet** `outlines.geoparquet` schema is exactly
  `basin_id: string, delineation: string, geometry: binary` (geoarrow.wkb), with a
  parquet `geo` key-value carrying `primary_column="geometry"` + CRS PROJJSON; **4
  rows** (≥2 `delineation` labels for a basin).
- **MS1 `Dtype` is a *closed* enum: `F32,F64,I32,I64,Bool,Timestamp` — there is NO
  `I8`.** The Zarr `era5_precipitation_was_filled` array is `int8`. This is the
  prior-iteration root-cause item: it MUST be fixed *at the root* in S1 by adding a
  `Dtype::I8` arm + an `int8` parse alias, never by a reader-local workaround.
- The `lib.rs` integration test asserts `every_core_error_variant_constructs`'s
  `variants.len() == 15`. New MS4 error variants change that count; the test is
  updated in lockstep in the step that introduces them.

### Crate-selection ground truth (R1, the gridded/geometry half)

| Need | Chosen reader | Rationale (verified) |
|---|---|---|
| Zarr v3 group/array metadata + consolidated metadata | **`serde_json` (already a dep)** parsing the store-root group `zarr.json` and per-array `zarr.json` | Consolidated metadata is a plain JSON block in one file; v0.1 needs metadata + 1-D coord reads, **not** a full array engine (architecture §1). Pulling the heavyweight `zarrs` stack for a JSON read + a 1-D coord decode is unjustified weight. |
| Zarr 1-D `lat`/`lon` coordinate chunk decode | **`ruzstd` (pure Rust), pinned** | The coord chunks are zstd-framed (`bytes`+`zstd` codec, magic `28 b5 2f fd`); decoding is unavoidable for the extent. The only in-tree zstd is `zstd 0.13` which **wraps `zstd-sys` (C)** — *not* pure Rust. A transitive dep cannot be `use`d; a **direct, pinned, pure-Rust** `ruzstd` is required (resolves prior MED-1). |
| COG tags + band descriptions + georef | **pure-Rust `tiff` (pinned major), GeoKey parse by hand** | `tiff 0.11.3` exposes `Tag::Unknown(42112)` + `get_tag_ascii_string`, reads IFD tags **without** decoding pixels (`read_image*` are separate explicit calls). The MED-4 three-outcome decision (below) governs the band-description read. |
| Geoparquet schema + columns + CRS | **the existing `arrow`/`parquet` stack (no new crate)** | geoparquet *is* parquet; the arrow schema gives the three columns; the parquet `geo` KV metadata gives `primary_column` + CRS. Reuses the proven MS3 reader. |

**This whole half stays pure-Rust, NO GDAL.** The only new third-party dependencies
are `ruzstd` (pinned) and `tiff` (pinned). Each is recorded as an architecture
Amendment in the step that adds it.

---

## MED-4 decision protocol (COG band description) — three named outcomes

Reading the per-band description (the spec-check **G1** "COG band description = field
name") from the multiband GeoTIFF via pure-Rust `tiff` is the highest-uncertainty
R1 dependency, because the description lives in the GDAL_METADATA (42112) XML, not a
native TIFF construct. MS4 makes an **explicit, recorded** decision in the
`architecture.md` Amendments log with exactly these three outcomes (decided in S3
against the real fixture; never silently):

1. **Pure-Rust read works** → the G1 COG-side band-name check is **metadata-deep and
   live**: the reader reads tag 42112, parses the `<Item name="DESCRIPTION"
   role="description">…</Item>` (and `units`) out of the GDAL XML, and round-trips
   the fixture's `"elevation"` band name + `"m"` units. (Expected outcome given the
   verified API.)
2. **Pure-Rust fails AND GDAL is accepted** → record the **GDAL system-dependency
   cost** as an amendment **AND confirm the MS9 maturin/PyO3 wheel still builds with
   it** before accepting. (Not the recommendation; documented only if 1 fails.)
3. **Pure-Rust fails AND GDAL is rejected** → the G1 COG band-name verification is
   reported as an **R3 byte/format-deep SKIP-with-reason**, never silently claimed
   as passing.

**NEVER silently reintroduce the GDAL system dependency.** **Round-trip rule:** S3
verifies the chosen reader actually reads the band description the MS2 generator
wrote (assert on the fixture). If it cannot, that is an **MS2 regenerate** (write the
description in a tag the reader supports), **never** a reader workaround.

## MED-5 decision protocol (Zarr consolidated metadata) — Rust-side confirmation

MS4 MUST confirm **from the Rust side** that the MS2 valid fixture's Zarr v3 store
exposes its metadata via the §8 **consolidated-metadata** path (one read to learn the
store). Two acceptable outcomes (decided in S2, recorded):

- **Live:** the reader reads the store via the store-root group `zarr.json`
  `consolidated_metadata` block (the verified fixture layout) and a Rust test asserts
  every array (`era5_precipitation`, `…_was_filled`, `lat`, `lon`, `time`, `crs`) is
  discoverable from that one read.
- **R3 skip:** consolidated-metadata / v3-sharding verification is classified as an
  **R3 byte-deep SKIP with a stated reason** — documented, never silently claimed.

**Regenerate, not workaround:** a `zarr-python`-vs-Rust mismatch is fixed by
**regenerating the MS2 fixture**, never a reader workaround.

## LOW-3 review gate (no gridded-chunk decode, no pixel raster) — asserted

It is a standing MS4 invariant, stated in every reader step and **asserted by a test
where feasible**, that **NO gridded-chunk decode and NO pixel-raster read happens
anywhere in `hdx-core`**:

- the Zarr reader reads only array **metadata** + the **1-D `lat`/`lon`/`time`
  arrays** (`lat`/`lon` decoded; `time` metadata-only per MED-2 below) + the CF
  `grid_mapping`/`crs` array — it **never** reads `era5_precipitation*/c/...` data
  chunks;
- the COG reader reads only IFD **tags** + band **metadata** + georef — it **never**
  calls `read_image*` / decodes a tile;
- the `gridded_*` subtrees are **opaque leaves** to the layout walk and
  **metadata-only** to the readers (architecture §1).

S2 and S3 each carry an explicit test that proves the data-chunk / pixel path is
untouched (e.g. the Zarr extent is identical whether or not the `c/0...` data chunk
files exist; the COG facts are read with `read_image` never invoked).

## MED-2 decision (Zarr `time` coordinate) — values NOT decoded in MS4, recorded

The Zarr **`time` coordinate VALUES are intentionally NOT decoded in MS4** — only its
**metadata** (shape/dtype/`units`/`calendar` from the consolidated `zarr.json`) is
read. Rationale: the dataset time axis is sourced from the parquet `time` column
(spec §6.2 / MS3), and no MS4 exit criterion (G1/G3/G2-precondition/Geo1/I1) needs
Zarr time *values*; the intra-basin Zarr-vs-parquet time-axis cross-check (spec-check
T2) is an **MS6** rule. The Zarr `GridExtent` therefore has **no time dimension**: it
is `west/north/x_res/y_res` derived from the decoded `lat`/`lon` values only. Every
coverage table in this plan says **"`lat`/`lon` coordinate values + `time` metadata"**
— never "`time` values" (resolves prior MED-2).

---

## Ordering rationale

Dependencies flow strictly downward; each step leaves the repo green.

1. **S1 — root types + shared model shells + the MED-4 decision frame.** Three things
   must precede any reader: (a) the **`Dtype::I8` root fix** (the `int8` mask array is
   unreadable until the closed enum admits it — fixing it in a reader would violate
   the "parse at the boundary, closed dtype" discipline); (b) the **shared gridded /
   geometry domain types** (`GridInfo`, `GridExtent`, gridded-`Field` plumbing,
   `delineation` model) and the **new typed error variants**, so S2–S5 slot in
   without reshaping the enum; (c) record the **R1 gridded/geometry crate decision**
   and the **MED-4 three-outcome frame** + **MED-5** + **MED-2** + **LOW-3**
   decisions in the architecture Amendments log as the recorded contract S2/S3 execute
   against. S1 has no reader logic, so it is the safe, reviewable floor. Every new
   field/getter/constructor is exercised by an S1 unit test (no `#[allow(dead_code)]`)
   so S1 is clippy-green standalone (resolves prior LOW-3).
2. **S2 — Zarr reader.** The Zarr metadata read (consolidated path, MED-5) and the
   1-D `lat`/`lon` coordinate decode (`ruzstd`, MED-1) is the densest reader; it
   produces the `GridInfo` for the dynamic grid and the CF-georef facts (G3) and the
   self-naming variable catalog (G1). Done first among readers because its
   `GridInfo`/`GridExtent` shapes are what the COG reader (S3) must produce
   *identically* (shared grid label ⇒ alignment precondition, G2).
3. **S3 — COG reader.** Reuses the `GridInfo`/`GridExtent` shapes from S2 and the
   MED-4 frame from S1; produces the static-grid `GridInfo`, band-description field
   catalog (G1), and georef facts (G3). Sequenced after S2 so the two readers' grid
   facts are comparable for the shared-label observation.
4. **S4 — geoparquet reader.** Independent of S2/S3 (reuses the MS3 parquet stack);
   produces the `delineation` labels + `basin_id`-in-outlines fact + outlines CRS
   (Geo1, I1-outlines). Placed after the gridded readers only so the combined model
   in S5 has all parts; it could equally precede them, but this keeps the "complete
   the discovery layer" assembly last.
5. **S5 — combined discovery assembler.** Ties S2/S3/S4 (keyed off the MS3 layout
   seam) into one `GriddedGeometryDiscovery` model that sits **alongside**
   `ScalarDiscovery`, exposing per-grid-label `GridInfo`, the gridded field catalog,
   the shared-grid-label-across-subtrees observation (G2 precondition), and the
   delineation labels. Resolves the subtree→artifact resolution (find the single
   `<label>.tif`/`<label>.zarr` in each subtree; resolves prior LOW-4). This is the
   step that makes the exit criterion "discovery layer complete" true.

This subdivides the milestone's single "readers" deliverable into one root-types
step + one step per physical format + one assembly step — each independently
committable and green, none doing a later milestone's enforcement work.

---

## Scope guard

Every step stays strictly inside MS4: it **reads and models** gridded/geometry
metadata into the shared discovery layer and **enforces no spec §14 check** (G1/G2/
G3/Geo1/I1 *enforcement* is MS6; `describe` assembly is MS5). No step introduces
`regrid`/`clip`/`reduce` or any reduction/hydrology operation (spec §10 — excluded
forever). No type or field added anywhere carries transform, role, semantic type, or
provenance (spec §1): `GridInfo`/`GridExtent`/gridded-`Field`/`delineation` are pure
structural facts; the six-field `Manifest` is untouched; no derivable field is added.
CRS is read **verbatim** from each file and only *recorded* — the spec-check M5
manifest-vs-file cross-check **rule** is deferred to MS6. **No gridded-chunk decode
and no pixel-raster read happens anywhere** (architecture §1); the `gridded_*`
subtrees are opaque leaves to the walk and metadata-only to the readers. The Zarr
`time` *values* are intentionally not decoded (MED-2). The hard-version-cut /
manifest boundary parse is **not** invoked here (that is the verbs' entry path, MS5/
MS6); MS4 readers operate over already-walked layout paths.

---

## MS4-S1 — Dtype `I8` root fix + shared gridded/geometry model shells + R1/MED-4 decision

**Intent.** Establish the floor the three readers stand on, with zero reader logic so
it is trivially reviewable and green standalone. (1) Fix the closed `Dtype` at the
root by adding `Dtype::I8` + an `int8`/`i8` parse alias (the Zarr mask array is
`int8`; a reader-local hack would violate the closed-dtype/parse-at-the-boundary
discipline). (2) Add the **shared gridded/geometry domain types** — `GridExtent`
(`west`/`north`/`x_res`/`y_res`, no time dim per MED-2), `GridInfo` (per grid-label:
`GridLabel`, `GridExtent`, CRS verbatim, field catalog), and the typed `CoreError`
variants the readers will raise (`ZarrRead`, `CogRead`, `GeoparquetRead`, plus the
"subtree does not contain exactly one artifact" variant) — so S2–S5 slot in without
reshaping the enum or the model. (3) Record, in `architecture.md` Amendments, the
**R1 gridded/geometry crate decision** (`serde_json`-for-Zarr-metadata + pinned
`ruzstd` + pinned `tiff` + reuse-parquet-for-geoparquet, no GDAL) and the **MED-4
three-outcome frame** + the **MED-5** and **MED-2** + **LOW-3** decisions as the
recorded contract S2/S3 execute against. Independently committable: pure types + docs
+ one amendment; the repo stays green because every new item is exercised by an S1
unit test.

**Changes.**
- `crates/core/src/field.rs` — add `Dtype::I8`; extend `Dtype::as_str` (`"i8"`) and
  `parse_dtype` aliases (`"i8" | "int8" => Dtype::I8`); add a unit test arm.
- `crates/core/src/grid.rs` (new) — `GridExtent` (`west,north,x_res,y_res: f64`,
  private fields + getters + `new`), `GridInfo` (`grid_label: GridLabel`,
  `extent: GridExtent`, `crs: Crs`, `fields: Vec<Field>`, getters + `new`), module
  `//!` doc. Inert/agnostic: every field a structural fact; no time dim (MED-2).
- `crates/core/src/error.rs` — add named-field variants `ZarrRead { artifact, detail }`,
  `CogRead { artifact, detail }`, `GeoparquetRead { artifact, detail }`,
  `AmbiguousGriddedArtifact { subtree, kind, found }` (fires when a `gridded_static`/
  `gridded_dynamic` subtree does not contain exactly one `<label>.tif`/`<label>.zarr`
  — resolves LOW-4), each doc-commented with *when* it fires; all inert (path/label +
  opaque detail only).
- `crates/core/src/lib.rs` — `pub mod grid;`; update the module-map `//!`; extend
  `every_core_error_variant_constructs` to construct the four new variants and bump
  the `variants.len()` assertion (15 → 19).
- `crates/core/Cargo.toml` — **no dependency change in S1** (readers add their pinned
  deps in their own steps, so each addition lands with its first consumer + its
  amendment).
- `architecture.md` — Amendments log: **R1 gridded/geometry decision** + **MED-4
  three-outcome frame** + **MED-5 Rust-side-confirmation contract** + **MED-2
  time-values-not-decoded** + **LOW-3 no-chunk/no-pixel invariant**. (Note: the task
  forbids modifying the spec; the architecture Amendments log is the living-doc home
  the milestone explicitly directs MS4 to use — this is the recorded-decision
  deliverable, not a spec change.)

**Test plan.**
- `parse_dtype("int8")` and `parse_dtype("i8")` → `Dtype::I8`; `Dtype::I8.as_str()`
  round-trips; the existing `parse_dtype_rejects_unknown_without_panicking` set still
  errors (e.g. `"u8"`, `"complex128"`).
- `grid.rs` unit tests: construct `GridExtent::new(10.0, 50.0, 0.25, 0.25)` and a
  `GridInfo` with one `Field`; assert every getter returns the constructed value
  (exercises every new field/getter/constructor → clippy-green standalone, no
  `#[allow(dead_code)]`; resolves LOW-3).
- `lib.rs`: `every_core_error_variant_constructs` constructs all 19 variants (incl.
  the four new ones) and asserts each `Display` is non-empty + `variants.len() == 19`.

**Acceptance.**
- `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings` all pass.
- `Dtype` is closed and admits `I8` via a fallible boundary parse (no panic/unwrap;
  CLAUDE.md) — the root fix for the `int8` mask array, not a reader workaround.
- `GridInfo`/`GridExtent` exist with private fields + getters, carry **no** time
  dimension (MED-2) and **no** transform/role/semantic/provenance field (spec §1).
- The four new error variants exist, are inert, and are exercised; the `lib.rs`
  count assertion is updated.
- `architecture.md` Amendments records: R1 gridded/geometry decision (pure-Rust, no
  GDAL); the MED-4 three-outcome frame; the MED-5 Rust-side-confirmation contract;
  the MED-2 time-values-not-decoded decision; the LOW-3 no-chunk/no-pixel invariant.
- Spec MUST-checks advanced (foundations; enforced MS6): typed plumbing for **G1**
  (gridded field catalog), **G3** (CRS/`GridInfo`), **H1** (gridded dtypes incl.
  `int8`).

**Spec refs.** §1 (inert/agnostic), §2 (closed dtype; ordinary fields), §7 (per-grid
extent/affine/res, one CRS), §8 (grid label), §14 G1/G3/H1 (foundations);
architecture §1, §3.3/§3.5, §7 R1/R3.

**Commit message.** `feat(core): add Dtype::I8 and shared GridInfo/GridExtent + gridded/geometry error variants (MS4-S1)`

---

## MS4-S2 — Zarr v3 metadata reader (consolidated path + 1-D lat/lon decode; time metadata-only)

**Intent.** Read one `gridded_dynamic/<label>.zarr` store into a `GridInfo` + a
gridded·dynamic `Field` catalog, **metadata + 1-D coordinate reads only**. The store
metadata is learned via the §8 **consolidated-metadata** path (MED-5 Rust-side
confirmation), parsed with `serde_json` from the store-root group `zarr.json`. The
self-naming CF variables (every non-coordinate, non-`crs` array → an ordinary
`GriddedDynamic` `Field` named verbatim, with `units` + `grid_label`) feed G1; the CF
`grid_mapping`→`crs` array (`spatial_ref`/`crs_wkt`) feeds G3 and the verbatim CRS.
The `GridExtent` (`west`/`north`/`x_res`/`y_res`) is derived **only** by decoding the
1-D `lat` and `lon` coordinate `c/0` chunks (zstd-framed → `ruzstd` → little-endian
f64). The `time` coordinate is read **metadata-only** (shape/dtype/`units`/`calendar`),
its **values intentionally not decoded** (MED-2). Independently committable: a
self-contained reader module with fixture-backed tests; the repo stays green.

**Changes.**
- `crates/core/Cargo.toml` — add **`ruzstd = "0.7"`** (pure Rust; pinned to its
  current major, mirroring the MS3 "pin the reader stack to exact majors" discipline)
  as a **direct** dependency with the rationale comment: *"decode the 1-D Zarr
  `lat`/`lon` coordinate chunks only (architecture §1) — the only in-tree `zstd` wraps
  `zstd-sys` (C), so a direct pure-Rust decoder is required; NO GDAL/C deps on this
  path."* (Resolves prior MED-1: pinned, direct, pure-Rust — not deferred.) No `zarrs`
  dependency: the consolidated metadata is a JSON read (`serde_json`, already a dep).
- `crates/core/src/zarr_reader.rs` (new) — `read_zarr(store_path) -> Result<GridInfo,
  CoreError>`:
  - parse the store-root group `zarr.json`; require its `consolidated_metadata`
    block (MED-5 live path) and enumerate member arrays from it;
  - classify members: coordinate arrays (`lat`,`lon`,`time` by `_ARRAY_DIMENSIONS`/
    `dimension_names` self-reference) + the `crs`/`grid_mapping` array vs ordinary CF
    variables (`era5_precipitation`, `era5_precipitation_was_filled` → `GriddedDynamic`
    `Field`s, dtype via the S1 closed `Dtype` incl. `int8`→`I8`, `units` from the CF
    `units` attr, `grid_label` from the store filename);
  - read CRS verbatim from the `grid_mapping` target array's `spatial_ref` (fallback
    `crs_wkt`) — recorded, not cross-checked (M5 rule is MS6);
  - **decode `lat` and `lon`** `c/0` chunks (`bytes`+`zstd` → `ruzstd` → f64) → derive
    `GridExtent { west=lon[0], north=lat[0], x_res=|lon[1]-lon[0]|, y_res=|lat[1]-lat[0]| }`;
  - read `time` **metadata only** (shape/dtype/`units`/`calendar`) — **do not decode
    its chunk** (MED-2);
  - typed `CoreError::ZarrRead` on a malformed/absent `zarr.json`, a missing
    `consolidated_metadata` block, an undecodable coord chunk, or fewer than two coord
    values (cannot derive a resolution).
- `crates/core/src/lib.rs` — `pub mod zarr_reader;`; extend the module-map `//!`.

**Test plan (against `conformance/valid/minimal/.../era5.zarr`).**
- **MED-5 (live):** a test asserts the store metadata is read via the
  `consolidated_metadata` path and that all members (`era5_precipitation`,
  `era5_precipitation_was_filled`, `lat`, `lon`, `time`, `crs`) are discovered from
  that single group-`zarr.json` read.
- **G1:** the variable catalog is exactly `{era5_precipitation: GriddedDynamic/F32,
  era5_precipitation_was_filled: GriddedDynamic/I8}`, each named verbatim
  (the `_was_filled` companion-mask name is an **ordinary** field — no suffix magic),
  each carrying `grid_label="era5"` and its `units` (`"mm"`, `"1"`).
- **G3 + CRS:** `GridInfo.crs()` is the verbatim `"EPSG:4326"` (from `spatial_ref`);
  the CF `grid_mapping`→`crs` array is present.
- **GridExtent (MED-1 decode live):** assert `west==10.0`, `north==50.0`,
  `x_res==0.25`, `y_res==0.25` — proving the `ruzstd` decode of the zstd-framed
  `lat`/`lon` chunks ran.
- **MED-2:** a test asserts the reader exposes `time` shape/dtype/`units` metadata and
  that **no `time` value array** is surfaced on `GridInfo` (the type has no time
  dimension).
- **LOW-3 (no data-chunk decode):** a test proves the data chunks are never read — copy
  the store to a temp dir, **delete the `era5_precipitation*/c/...` data-chunk files**
  (leaving `zarr.json` + `lat/lon/time` coord chunks), and assert `read_zarr` still
  returns the identical `GridInfo` (extent, catalog, CRS) — the data chunks demonstrably
  do not participate.
- **Negative:** a store dir whose group `zarr.json` lacks `consolidated_metadata`, and
  a store with a corrupt coord chunk, each return `CoreError::ZarrRead` (no panic).

**Acceptance.**
- `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings` pass.
- `ruzstd` is a **direct, pinned, pure-Rust** dependency with the metadata/1-D-coord
  rationale recorded in `Cargo.toml` and the S1 R1 amendment; **no GDAL/C** on this
  path (resolves MED-1).
- **MED-5 resolved live:** the reader reads the §8 consolidated-metadata path on the
  fixture, asserted by a Rust test (or, if it could not, an R3 byte-deep skip recorded
  with a reason — here it is live).
- **MED-2 honored:** Zarr `time` is metadata-only; values not decoded; `GridExtent`
  has no time dim; coverage wording is "`lat`/`lon` values + `time` metadata".
- **LOW-3 honored:** the no-data-chunk-decode test passes; the reader touches only
  metadata + the 1-D `lat`/`lon` coordinate chunks (+ `time` metadata).
- Spec MUST-checks advanced (foundations; enforced MS6): **G1** (self-naming CF
  variables, no positional channel axis), **G3** (CF `lat`/`lon` + `grid_mapping`
  georef present), **H1** (gridded dtypes incl. `int8`). G2 precondition (the dynamic
  grid's `GridInfo`) is produced for S5's shared-label observation.

**Spec refs.** §7 (per-variable native grid, CF georef, one CRS), §8 (one
artifact=one grid; self-naming variables; consolidated metadata; sharding), §2
(ordinary fields incl. companion mask), §14 G1/G3/H1 (foundations); architecture §1,
§7 R1/R3.

**Commit message.** `feat(core): read Zarr v3 consolidated metadata + 1-D lat/lon coords into GridInfo (MS4-S2)`

---

## MS4-S3 — COG band-description + georef reader (MED-4 round-trip; no pixel read)

**Intent.** Read one `gridded_static/<label>.tif` multiband COG into a `GridInfo` + a
gridded·static `Field` catalog, **IFD tags + band metadata + georef only — never a
pixel/tile decode** (LOW-3). Band descriptions (= field names → `GriddedStatic`
`Field`s) and band units come from the **GDAL_METADATA tag 42112** XML; the
`GridExtent`/CRS come from the standard georef tags (33550 ModelPixelScale, 33922
ModelTiepoint, 34735 GeoKeyDirectory). Execute the **MED-4 three-outcome decision**
against the real fixture and record which outcome held. Independently committable: a
self-contained reader module with fixture-backed tests; repo stays green.

**Changes.**
- `crates/core/Cargo.toml` — add **`tiff = "0.11"`** (pure Rust; pinned major) as a
  direct dependency with the rationale: *"read COG IFD tags + GDAL band metadata +
  georef tags only (architecture §1) — never decode pixels; NO GDAL system dep."*
- `crates/core/src/cog_reader.rs` (new) — `read_cog(tif_path, grid_label) ->
  Result<GridInfo, CoreError>`:
  - open with `tiff::decoder::Decoder::new` (reads the IFD/tags, **not** pixels);
  - **band descriptions + units:** `get_tag_ascii_string(Tag::Unknown(42112))` →
    parse the GDAL XML `<Item name="DESCRIPTION" sample="N" role="description">…</Item>`
    (band name) and `<Item name="units" sample="N">…</Item>` per band → one
    `GriddedStatic` `Field` per band (verbatim name, dtype `F32` from SampleFormat/
    BitsPerSample → S1 `Dtype`, `units`, `grid_label`). MED-4 outcome (1) path.
  - **georef:** `ModelPixelScaleTag` (33550) → `x_res`/`y_res`; `ModelTiepointTag`
    (33922) → `west`/`north`; `GeoKeyDirectoryTag` (34735) → EPSG code → CRS
    `"EPSG:<code>"` verbatim (recorded, not cross-checked — M5 is MS6) →
    `GridExtent`/`GridInfo`;
  - typed `CoreError::CogRead` on an unreadable TIFF, an absent/unparsable 42112 tag
    (subject to MED-4: see acceptance), or missing georef tags. **`read_image*` is
    never called.**
- `crates/core/src/lib.rs` — `pub mod cog_reader;`; extend the module-map `//!`.
- `architecture.md` — Amendments: record the **realized MED-4 outcome** (expected (1):
  pure-Rust read of tag 42112 works; the G1 COG-side check is metadata-deep and live),
  with the round-trip evidence; if outcome (1) does not hold, record (2) or (3) per the
  S1 frame (never silently GDAL).

**Test plan (against `conformance/valid/minimal/.../era5.tif`).**
- **MED-4 round-trip (G1 COG-side):** assert the band catalog is exactly
  `{elevation: GriddedStatic/F32, units "m", grid_label "era5"}` — read out of the
  42112 GDAL XML. This is the explicit round-trip on the fixture the MED-4 protocol
  demands; a failure here is an **MS2 regenerate**, not a reader workaround.
- **G3 + CRS + extent:** `GridInfo.crs() == "EPSG:4326"` (from GeoKeyDirectory);
  `GridExtent` `west==10.0`, `north==50.0`, `x_res==0.25`, `y_res==0.25` (from
  tiepoint + pixel-scale) — matching S2's Zarr extent (the alignment the G2 positive
  path later checks in MS6).
- **LOW-3 (no pixel read):** a test asserts the facts are obtained without invoking
  `read_image*`; structurally, `read_cog` has no call to any pixel-decode method
  (review + a test that the reader succeeds on the tags alone). Where feasible, prove
  pixel-independence by reading facts from the IFD only.
- **Negative:** a non-TIFF file and a TIFF with no 42112 tag each surface
  `CoreError::CogRead` (no panic). The no-42112 case is *not* the committed fixture
  (42112 is present) — were it ever to become so, that is the MED-4 (3) skip path,
  documented, never a silent claim.

**Acceptance.**
- `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings` pass.
- `tiff` is a **direct, pinned, pure-Rust** dependency with the tags-only rationale; no
  GDAL system dep.
- **MED-4 resolved + recorded:** the realized outcome is written to the architecture
  Amendments log; the band-description round-trip on the fixture passes (outcome (1)),
  OR outcome (2)/(3) is recorded per the S1 frame — **never silently GDAL, never a
  silent claim**. A reader/writer mismatch would be an MS2 regenerate.
- **LOW-3 honored:** no pixel/tile decode; `read_image*` never called.
- Spec MUST-checks advanced (foundations; enforced MS6): **G1** (COG band
  description = field name; no positional channel axis), **G3** (GeoTIFF georef tags
  present). G2 precondition: the static grid's `GridInfo` (extent/CRS) is produced for
  S5's shared-label-across-subtrees observation.

**Spec refs.** §7 (GeoTIFF georef tags, one CRS), §8 (multiband COG; band
description = field name; grid label), §2 (ordinary fields), §14 G1/G3 (foundations);
architecture §1, §7 R1/R3.

**Commit message.** `feat(core): read COG band descriptions (GDAL tag 42112) + georef into GridInfo (MS4-S3)`

---

## MS4-S4 — geoparquet reader (outlines schema + delineation + basin_id + CRS)

**Intent.** Read the dataset-level `outlines.geoparquet` into typed facts: the schema
shape `(basin_id, delineation, geometry)`, the discovered `delineation` labels, the
`basin_id`-in-outlines presence (I1-outlines), the outlines CRS (verbatim), and the
"single file at root, not partitioned by delineation" structural fact (Geo1) — reusing
the **existing `arrow`/`parquet` stack** (geoparquet *is* parquet; no new crate, no
full-geometry decode). Independently committable: a self-contained reader over the
proven MS3 metadata path; repo stays green.

**Changes.**
- `crates/core/src/geoparquet_reader.rs` (new) — `read_outlines(path) ->
  Result<OutlinesInfo, CoreError>` where `OutlinesInfo` (new, inert) carries:
  `has_basin_id: bool`, `has_delineation: bool`, `has_geometry: bool` (the Geo1 schema
  shape), `delineations: Vec<DelineationLabel>` (distinct values via a bounded
  `delineation`-only 1-D column read, reusing the MS3 projection pattern), and
  `crs: Option<Crs>` (verbatim from the parquet `geo` KV metadata `primary_column`
  CRS PROJJSON / the geometry-field metadata — recorded, not cross-checked). It reads
  the arrow schema + the `delineation` column + the `geo` KV; it **never decodes WKB
  geometries**.
  - typed `CoreError::GeoparquetRead` on a missing/undecodable file or a `delineation`
    column that is not a string array.
- `crates/core/src/lib.rs` — `pub mod geoparquet_reader;`; extend the module-map `//!`.

**Test plan (against `conformance/valid/minimal/outlines.geoparquet`).**
- **Geo1 schema:** `has_basin_id && has_delineation && has_geometry` all true; the
  label column is named `delineation`.
- **Delineations:** the discovered `delineations` are the distinct labels in the
  fixture (≥2 for at least one basin per §9 plurality), each an opaque
  `DelineationLabel` (no interpretation).
- **I1-outlines:** `basin_id` column present.
- **CRS:** the outlines CRS is read verbatim from the `geo` KV metadata (recorded, not
  cross-checked — M5 is MS6).
- **No geometry decode:** the reader surfaces all facts without decoding the WKB
  `geometry` column (a `delineation`-only bounded read; review + assertion that the
  geometry column is never materialized).
- **Negative:** a non-parquet file and a parquet missing the `delineation` column each
  return `CoreError::GeoparquetRead` (no panic).

**Acceptance.**
- `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings` pass.
- No new dependency (reuses the MS3 `arrow`/`parquet` stack); the reader is
  metadata + 1-D-column only — no WKB decode (architecture §1).
- `OutlinesInfo` is inert (schema-shape booleans + opaque labels + verbatim CRS); no
  transform/role/semantic/provenance.
- Spec MUST-checks advanced (foundations; enforced MS6): **Geo1** (outlines schema
  `(basin_id, delineation, geometry)`, label column `delineation`, single root file
  not partitioned), **I1** (`basin_id` present in outlines). CRS read for the MS6 M5
  cross-check.

**Spec refs.** §9 (plural outlines, `delineation` column, not partitioned, neutral
labels), §3 (`basin_id` column), §7/§11 (one CRS), §14 Geo1/I1 (foundations);
architecture §1, §5, §7 R1.

**Commit message.** `feat(core): read outlines.geoparquet schema + delineation/basin_id columns + CRS (MS4-S4)`

---

## MS4-S5 — combined gridded/geometry discovery + subtree→artifact resolution (discovery layer complete)

**Intent.** Tie S2/S3/S4 into one `GriddedGeometryDiscovery` model that sits
**alongside** `ScalarDiscovery` (architecture §3.5), keyed off the MS3 layout seam
(`BasinDir::gridded_static()`/`gridded_dynamic()` + `RootRollupPresence::
outlines_present()`). It walks each basin's gridded subtrees, **resolves the single
`<label>.tif` / `<label>.zarr` inside each subtree** (deriving `grid_label` from that
filename, and surfacing `CoreError::AmbiguousGriddedArtifact` if a subtree does not
contain exactly one such artifact — resolves LOW-4), invokes the S2/S3/S4 readers, and
assembles: per-grid-label `GridInfo` (extent/affine/res/CRS), the gridded field
catalog, the **shared-grid-label-across-subtrees observation** (the on-disk G2
precondition: the static COG and dynamic Zarr share label `"era5"`, read but not
enforced), and the `delineation` labels. After this step the **discovery layer is
complete** — both verbs (MS5/MS6) can assemble from one typed in-memory model.
Independently committable: the assembler + a fixture-backed end-to-end test; repo stays
green.

**Changes.**
- `crates/core/src/discovery.rs` — add the gridded/geometry half **alongside**
  `ScalarDiscovery` (no reshape of the scalar model):
  - a subtree→artifact resolver `resolve_single_artifact(subtree, extension) ->
    Result<(PathBuf, GridLabel), CoreError>` that enumerates a `gridded_static`/
    `gridded_dynamic` subtree, requires **exactly one** `<label>.tif`/`<label>.zarr`,
    derives `grid_label` from the filename stem, and returns `AmbiguousGriddedArtifact`
    otherwise (resolves LOW-4; the analogous resolution for the `.zarr` store is the
    same function with the `zarr` extension);
  - `GriddedGeometryDiscovery` (new, inert): `grids: Vec<GridInfo>` keyed by grid
    label, `gridded_fields: Vec<Field>` (the homogeneous gridded catalog, one-basin
    read per §5), `shared_labels: Vec<GridLabel>` (labels observed in **both** the
    static and dynamic subtrees — the G2 precondition fact), `delineations:
    Vec<DelineationLabel>`, getters for each;
  - `discover_gridded_geometry(path) -> Result<GriddedGeometryDiscovery, CoreError>`:
    walk the layout, for a representative basin resolve + read its `gridded_static`
    (S3) and `gridded_dynamic` (S2) artifacts into `GridInfo`s, observe the shared
    label across the two subtrees, and (if `outlines_present`) read S4's outlines into
    the delineation labels. **Surfaces gaps as facts, never a verdict** (a basin with
    no gridded subtree yields no gridded grids; absent outlines yields empty
    delineations) — mirroring the scalar half's discipline.
- `crates/core/src/lib.rs` — extend the `discovery` module-map `//!` to describe the
  now-complete discovery layer (scalar half + gridded/geometry half).

**Test plan (against `conformance/valid/minimal/`).**
- **Subtree→artifact resolution (LOW-4):** a unit test over a temp subtree with two
  `.tif`s returns `AmbiguousGriddedArtifact`; the real fixture (one `era5.tif`, one
  `era5.zarr` per basin) resolves to `grid_label="era5"` for both.
- **End-to-end:** `discover_gridded_geometry("valid/minimal")` yields: one Zarr
  `GridInfo` (`era5`, extent 10/50/0.25/0.25, EPSG:4326, fields era5_precipitation +
  _was_filled) and one COG `GridInfo` (`era5`, same extent, EPSG:4326, field
  elevation); `gridded_fields` is the homogeneous gridded catalog;
  `shared_labels == ["era5"]` (the G2 precondition observed across subtrees);
  `delineations` is the fixture's distinct label set.
- **G2 precondition observed, not enforced:** the test asserts the shared label is
  *observed* across the two subtrees and that the two `GridInfo`s' extents coincide,
  documenting (not enforcing) the cell-for-cell alignment MS6's G2 rule will check.
- **LOW-3 (whole-half no-chunk/no-pixel):** an assertion (carried from S2/S3) that the
  combined assembly reads only metadata + 1-D coords + tags — exercised by reusing the
  S2 no-data-chunk temp-store test through the combined path.
- **Gaps-as-facts:** running over `invalid/missing-root-rollup` (outlines absent)
  succeeds with empty `delineations` and the gridded grids still discovered (no
  verdict; Geo1/L1 enforcement is MS6).

**Acceptance.**
- `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings` pass.
- The discovery layer is **complete**: a single typed in-memory model (`ScalarDiscovery`
  + `GriddedGeometryDiscovery`) is available for MS5/MS6 to assemble from.
- Subtree→artifact resolution is explicit: each gridded subtree must contain exactly
  one `<label>.tif`/`<label>.zarr`; otherwise `AmbiguousGriddedArtifact` (resolves
  LOW-4). `grid_label` is derived from the artifact filename.
- The shared-grid-label-across-subtrees fact is **observed** (G2 precondition), not
  enforced; CRS recorded verbatim (M5 deferred to MS6).
- **LOW-3** holds for the whole half: no gridded-chunk decode, no pixel read anywhere.
- Spec MUST-checks advanced (foundations; enforced MS6): **G1**, **G2** (precondition:
  shared label observed across subtrees), **G3**, **Geo1**, **I1** (basin_id in
  outlines). The milestone exit criterion "discovery layer complete" is satisfied.

**Spec refs.** §5 (one-basin discovery), §7 (per-grid info, one CRS), §8 (one
artifact=one grid; shared label ⇒ alignment), §9 (delineation labels), §14
G1/G2/G3/Geo1/I1 (foundations); architecture §1, §3.5, §5, §7 R1/R3.

**Commit message.** `feat(core): assemble gridded+geometry discovery half and complete the discovery layer (MS4-S5)`

---

## Coverage — every MS4 deliverable / exit criterion / spec ref is assigned

| MS4 deliverable / exit criterion | Step(s) |
|---|---|
| R1 gridded/geometry crate decision recorded (architecture amendment) | S1 (frame) + S2 (`ruzstd`) + S3 (`tiff`) + S4 (reuse parquet) |
| Zarr reader: per-grid-label metadata, self-naming variables (G1) | S2 |
| Zarr CF `lat`/`lon` coordinate **values** + `grid_mapping`/CRS + extent/affine/res (G3) | S2 |
| Zarr `time` **metadata** (values intentionally not decoded, MED-2) | S2 |
| §8 consolidated-metadata read live (MED-5 Rust-side confirmation) | S2 |
| COG reader: band descriptions = field names (G1), georef tags (G3) | S3 |
| MED-4 three-outcome decision recorded + fixture round-trip; never silently GDAL | S1 (frame) + S3 (execute + record) |
| Geoparquet reader: `(basin_id, delineation, geometry)` schema, `delineation` label, basin_id, not partitioned (Geo1, I1) | S4 |
| Unified discovery model (`GridInfo` per label, delineations, gridded catalog) alongside scalar half | S1 (types) + S5 (assembly) |
| Shared-grid-label-across-subtrees observation (G2 precondition) | S5 |
| Discovery layer complete (both verbs can assemble) | S5 |
| `int8` Zarr dtype admitted at the root (`Dtype::I8`) | S1 |
| LOW-3: no gridded-chunk decode, no pixel read (asserted) | S2 + S3 + S5 |
| CRS read verbatim from each file; M5 cross-check deferred to MS6 | S2/S3/S4 (read) → MS6 (rule) |
| `cargo build` + `cargo test` + `cargo clippy --all-targets -- -D warnings` after each step | S1–S5 |
| Commit via bump+tag convention | S1–S5 (each) |

**Spec §14 checks advanced (foundations only — enforcement is MS6):** G1 (S2/S3),
G2-precondition (S5), G3 (S2/S3), Geo1 (S4), I1-outlines (S4), H1-gridded-dtypes (S1).
No MS4 step *enforces* any check; all enforcement is MS6, `describe` assembly is MS5.
