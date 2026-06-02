# HDX v0.1 — Build Architecture

> **Purpose.** A build-oriented distillation of `spec/HDX_SPEC.md` (the canonical
> spec). This file is what every milestone and step plan is *planned against*.
>
> **This is a LIVING document.** Unlike the spec (which is the settled contract),
> the architecture is expected to change as the build proceeds: friction,
> problems, surprising crate behavior, and recurring issues SHOULD be folded back
> in here as amendments — that is the point of the file. Record what was learned
> next to the decision it revises (an `Amendments` log at the bottom is the
> conventional home). The one hard constraint: it MUST NOT contradict the spec —
> on any conflict the spec wins and this file is the bug. Everything else here is
> revisable guidance, not contract.
>
> Scope of v0.1 = **`validate` + `describe`** in `hdx-core`, a thin JSON CLI over
> them, and (last) a PyO3 binding. **`regrid`/`clip`/`reduce` are EXCLUDED.**

---

## 1. The central build insight — read metadata, not chunks

`validate` and `describe` answer questions about the **shape and structure** of
data, never its scientific values. Almost every conformance check and every
discovered fact is available from **metadata and small index reads**, not from
decoding gridded chunks:

| Need | Source (cheap) | Not needed |
|---|---|---|
| field catalog, units, dtypes | parquet schema; Zarr array metadata + attrs; COG band descriptions | chunk data |
| per-field grid (extent/affine/res) | Zarr `lat`/`lon` coord arrays + `grid_mapping`; GeoTIFF georef tags | pixel values |
| time ranges & alignment | parquet `time` column; Zarr `time` coordinate array (1-D, small) | gridded chunks |
| basin list & identity | `basin=<id>` dirs; `basin_id` columns | — |
| delineation labels | `outlines.geoparquet` `delineation` column | full geometries |
| CRS / cadence cross-check | manifest + file georef metadata + time axis | — |

**Consequence:** v0.1 needs **format readers for metadata + 1-D coordinate /
key-column reads**, not full array engines. This is what makes `validate` /
`describe` tractable in Rust without GDAL-scale dependencies, and it is the
backbone of the milestone sequencing (§6). Deep byte-level checks are explicitly
*incremental* (spec §14 note): the validator MUST report which checks ran.

---

## 2. Crate / package structure (build into the EXISTING scaffold)

The workspace already exists — do **not** re-scaffold. Build into it:

```
hdx/                              # repo root (= the "hdx/" in doc paths)
  Cargo.toml                      # [workspace] members=crates/* ; root bin pkg `hdx` (CLI) v0.1.x
  src/main.rs                     # the thin `hdx` CLI  → wraps hdx-core verbs, emits JSON
  crates/
    core/                         # hdx-core (lib) — ALL contract logic lives here
      Cargo.toml                  # thiserror, tracing (+ IO deps added per-milestone)
      src/lib.rs
      README.md                   # crate entry-point doc (Mermaid module map + glossary)
    python/                       # crates/python — PyO3 binding (LAST milestone; maturin)
  spec/HDX_SPEC.md                # canonical spec
  architecture.md                 # this file
  schemas/                        # JSON Schema for manifest.json (+ describe output schema)
  conformance/                    # fixture datasets (valid + invalid) + golden describe outputs
  planning/                       # milestone & step plans + critiques (orchestration artifacts)
```

**Placement rules (from spec §10/§13):**

- ALL contract logic (`validate`, `describe`, the type model, the format
  readers) lives in **`hdx-core`**. The spec and its validator are the same
  artifact.
- The root `hdx` bin is **thin glue only**: arg parsing → call `hdx-core` →
  serialize result to JSON → exit code. No contract logic in `main.rs`.
- `crates/python` mirrors `validate`/`describe` over the same `hdx-core` API.
- **Nothing** in this repo implements `regrid`/`clip`/`reduce` or the blessed
  reduction. If a plan proposes them, it is out of scope — reject.

**Version/commit discipline (CLAUDE.md / AGENTS.md, non-negotiable):** every
commit runs `./scripts/bump-version.sh patch`, stages `Cargo.toml`, commits with
a conventional message, then `git tag v<version>`. `tracing` only — never
`println!` (the CLI emits JSON via `serde_json` to stdout, which is *output*, not
logging; diagnostics go through `tracing` to stderr). Edition 2024.

---

## 3. The type model (parse-don't-validate, types as the floor)

All raw input (paths, JSON, parquet/zarr/tiff bytes) is parsed into typed domain
representations **at the boundary**; internal logic only ever sees valid-by-
construction types. Sketch (final shapes are a milestone deliverable — the
planner refines, but it MUST honor these invariants):

### 3.1 Newtypes (confusion-prone values get distinct types)

```rust
pub struct BasinId(String);          // unique within a dataset (§3); opaque
pub struct FieldName(String);        // opaque producer string (§2); HDX parses none
pub struct GridLabel(String);        // names a grid family; shared label ⇒ alignment (§8)
pub struct DelineationLabel(String); // neutral label (§9); not "hydrofabric"
pub struct Crs(String);              // e.g. "EPSG:4326"
pub struct Cadence(String);          // e.g. "daily"
pub struct DatasetName(String);
pub struct ProducerVersion(String);
```

### 3.2 The hard version cut

```rust
/// The ONLY contract version axis. Hard cut: unknown ⇒ reject before anything else.
pub enum FormatVersion { V0_1 }      // parsing "0.1" succeeds; any other string errors
```

### 3.3 The field 2×2 — enums, never booleans

```rust
pub enum Temporal { Static, Dynamic }   // a value, or a series
pub enum Shape    { Scalar, Gridded }   // a single value, or a per-cell field

/// The four quadrants (Temporal × Shape). The unit of HDX is the field.
pub enum Quadrant { ScalarStatic, ScalarDynamic, GriddedStatic, GriddedDynamic }

pub enum Dtype { /* f32,f64,i32,i64,bool,timestamp,… opaque to semantics */ }

pub struct Units(Option<String>);       // units or none — opaque string, no parsing

pub struct Field {
    name: FieldName,
    quadrant: Quadrant,
    dtype: Dtype,
    units: Units,
    grid_label: Option<GridLabel>,       // Some iff Shape::Gridded
}
```

**The quadrant is a property of each field, never of the dataset (spec §2).** A
dataset's schema is a `Vec<Field>` that MAY mix all four quadrants freely — e.g.
`gridded·dynamic` forcing + `scalar·dynamic` streamflow + `scalar·static`
attributes in one dataset. The discovery layer therefore derives **which
physical artifacts must exist** from the field set: a `gridded·dynamic` field
implies a `gridded_dynamic/<label>.zarr`; a dataset with no gridded fields has no
`gridded_*` subtrees at all. `validate` checks artifacts-present against
fields-declared, not against any fixed dataset "mode".

### 3.4 The manifest — exactly the six floor fields (§11)

```rust
pub struct Manifest {
    format_version: FormatVersion,       // read & cut FIRST
    name: DatasetName,
    created_at: OffsetDateTime,          // RFC 3339
    producer_version: ProducerVersion,
    crs: Crs,
    cadence: Cadence,
}
```

Parsing rejects any *extra* (derivable) field — adding one is a conformance bug.

### 3.5 Discovery & report types (the verb outputs)

```rust
/// What `describe` returns — the full self-description, all DISCOVERED (§10/§11).
pub struct Description {
    manifest: Manifest,
    basins: Vec<BasinId>,
    fields: Vec<Field>,                  // the homogeneous schema (one-basin read, §5)
    grids: Vec<GridInfo>,                // per grid-label: extent/affine/res/crs (representative)
    time_extent: Vec<BasinTimeExtent>,   // per-basin ragged [start,end] (§6.1)
    delineations: Vec<DelineationLabel>, // discovered from outlines (§9)
}

/// What `validate` returns — every check, with outcome (machine-readable).
pub struct ValidationReport {
    checks: Vec<CheckOutcome>,           // id (M1, L2, T2, G1, …), ran/skipped, pass/fail, detail
    conformant: bool,                    // true iff every applicable MUST passed
}
```

`describe` and `validate` share one **discovery layer** (open dataset → typed
in-memory model); `describe` *reports* it, `validate` *checks rules over it*.

### 3.6 Errors (thiserror; library code, no `unwrap`/`expect`)

`hdx-core` uses `thiserror` with named-field variants, each doc-commented with
*when* it fires (e.g. `UnknownFormatVersion`, `BasinIdFolderMismatch`,
`NonMonotonicTime`, `RaggedSchema`, `GridLabelMismatchAcrossBasins`,
`MissingRootRollup`). The CLI/glue (`src/main.rs`) uses `anyhow` with `.context`.

---

## 4. On-disk layout (authoritative reference — see spec §4)

```
<hdx-dataset>/
  manifest.json                       # six floor fields (§11)
  scalar_static.parquet               # dataset-level rollup; 1 row/basin; basin_id + static scalar fields
  outlines.geoparquet                 # dataset-level; rows (basin_id, delineation, geometry)
  basin=<id>/
    scalar_dynamic.parquet            # rows = `time` (timestamp, sorted, non-null); basin_id + dynamic scalar fields
    gridded_static/<grid-label>.tif   # multiband COG; band description = field name
    gridded_dynamic/<grid-label>.zarr # Zarr v3; named CF variable = field name; CF lat/lon + grid_mapping
  basin=<id>/ …
```

Asymmetry is principled (size/shape, not convention): `scalar_static` + outlines
roll up to the root; large per-basin data stays under `basin=<id>/`.

---

## 5. `validate` vs `describe` responsibilities

| | `describe` (discovery) | `validate` (conformance) |
|---|---|---|
| **Goal** | Emit the full self-description discovered from files | Decide conformance against the spec `MUST` set (§14) |
| **Reads** | Manifest + one-basin schema + per-grid metadata + outlines labels + per-basin time extents | Same discovery layer + cross-checks |
| **Output** | `Description` → JSON | `ValidationReport` (per-check outcomes + `conformant`) → JSON |
| **Failure mode** | Surfaces what's there; reports gaps as facts | Fails closed on any violated `MUST`; reports which checks ran |
| **Shared** | One discovery layer (open → typed model). `describe` is the stress test of the manifest floor — if it's hard, the floor is too thin. | |

Mapping of the spec §14 checklist to where it runs:

- **Manifest M1–M6** — manifest parse + CRS/cadence cross-check against files.
- **Layout L1–L3** — directory walk; rollups at root; `basin=<id>` shape; no
  ragged files (absence = NaN, not missing file).
- **Identity I1–I3** — `basin_id` column present, agrees with folder, unique.
- **Homogeneity H1–H2** — one-basin schema vs every basin; identical grid-label
  set.
- **Time T1–T2** — `time` column type/sort/non-null; intra-basin axis alignment
  (parquet `time` vs each Zarr `time` coord).
- **Grids G1–G3** — one-artifact-one-grid; self-naming (no positional channel
  axis); grid-label naming + shared-label alignment; CF / GeoTIFF georef present.
- **Geometry Geo1** — `outlines.geoparquet` schema `(basin_id, delineation,
  geometry)`; label column `delineation`; not partitioned by delineation.

---

## 6. Suggested milestone sequencing (the planner owns the final cut)

This is a **build-tractability hint**, not the plan. The PLANNER/CRITIC loop
produces the authoritative `planning/milestones.md`. Dependencies flow downward.

1. **Core types + manifest.** Newtypes, `FormatVersion` hard cut, field 2×2 /
   `Quadrant`, `Dtype`/`Units`, `Manifest` parse (reject extras), error enum.
   Unit-tested with no external IO. *Also: manifest JSON Schema → `schemas/`.*
2. **Layout discovery + scalar parquet.** Walk the tree; enumerate basins; read
   parquet schema + `basin_id` + `time` column (arrow/parquet). Builds the
   discovery layer's scalar half.
3. **Gridded + geometry metadata readers.** Zarr v3 metadata/attrs + `time`/
   `lat`/`lon` coords (`zarrs`); COG band descriptions + georef tags; geoparquet
   schema + `delineation`/`basin_id` columns. Completes the discovery layer.
4. **`describe`.** Assemble `Description` from the discovery layer; JSON output;
   golden outputs in `conformance/`.
5. **`validate`.** Implement the §14 `MUST` checklist over the discovery layer;
   `ValidationReport`; report which checks ran.
6. **The `hdx` CLI.** Thin `hdx validate <path>` / `hdx describe <path>`; JSON to
   stdout; non-zero exit on non-conformance; `tracing` to stderr.
7. **Conformance suite.** Curated valid + invalid fixtures (each invalid pins one
   violated check id), golden `describe` JSON, regression tests.
8. **PyO3 binding.** `crates/python` (maturin) mirroring `validate`/`describe`.

Each milestone is vertically meaningful and independently reviewable; steps
within a milestone are dependency-sequential.

---

## 7. Build risks & decisions the planner/critic MUST resolve

These are flagged here so the planning loop confronts them early; the
architecture recommends a default but the PLANNER decides and the CRITIC
scrutinizes:

- **R1 — Format-reader crate selection.** Candidates: `arrow`/`parquet`
  (parquet, mature ✓); `zarrs` (Zarr v3 incl. sharding + consolidated metadata);
  geometry via `geoarrow`/`wkb`/`geo-types`; COG via pure-rust `tiff` +
  GeoKey parsing **vs** `gdal` bindings. *Recommended default:* pure-Rust stack
  (avoid the GDAL system dependency); fall back to `gdal` only if a required
  metadata read (e.g. COG band descriptions) is otherwise unreachable. Decide in
  Milestone 3 planning, but surface at milestone-planning time.
- **R2 — Fixture generation (no HDX writer exists in v0.1).** `validate`/
  `describe` are read-only, yet the conformance suite needs real Zarr/COG/
  parquet/geoparquet datasets. *Recommended default:* a dev-only fixture
  generator (Python: `pyarrow` + `xarray`/`zarr` + `rioxarray` + `geopandas`),
  checked into `conformance/` with a `make`-style regenerate script — NOT part of
  shipped `hdx-core`. Alternative: Rust test helpers using the write features of
  the same reader crates. The planner MUST pick one before Milestone 4/7.
- **R3 — Depth of byte-level checks for v0.1.** The spec permits incremental
  enforcement (§14 note). The planner decides which `MUST` checks are
  metadata-deep (always) vs byte-deep (e.g. verifying actual sharding/overviews),
  and the validator MUST report skipped checks honestly.
- **R4 — `describe` output schema stability.** Since LLMs and PyO3 consume it,
  the `Description` JSON shape is itself a mini-contract; define it (and pin a
  JSON Schema in `schemas/`) in Milestone 4, not ad hoc.

None of these may compromise the spec invariants: HDX stays **inert** (no
transform/role/semantic/provenance), the **manifest floor** holds (only the six
non-derivable fields), `format_version` is a **hard cut**, and parsing happens
**at the boundary** (invalid states unrepresentable downstream).

---

## 8. Amendments log

This file is living (see the header). When the build surfaces friction, a wrong
assumption, a crate that doesn't behave as planned, or a recurring issue, record
it here — newest first — with the date, what changed, and why. Each entry should
let a future agent understand a decision that the body text above now reflects.

| Date | Amendment | Why |
|---|---|---|
| 2026-06-02 | **MS8 conformance-suite tracking policy: fixtures git-ignored + goldens relocated.** Two coupled decisions, now in effect (since v0.1.48 for the gitignore, v0.1.50 for the relocation). **(1) Generated fixture DATA is git-ignored, regenerated deterministically.** Everything under `conformance/valid/**` and `conformance/invalid/**` is git-ignored and is **not** committed; it is reproduced bit-for-bit by the committed generator (`conformance/generator/`), run via `PYTHON=python3.12 conformance/generator/regenerate.sh` before `cargo test`. The **tracked** artifacts are exactly: the generator source, `conformance/goldens/*.json` (the Rust-verb-produced assertion baselines), `conformance/README.md`, the Rust tests, and `Cargo.toml`/`Cargo.lock`. *Why:* keep binary fixture blobs (parquet/Zarr/COG/geoparquet bytes — every regenerate rewrites them) out of git history, while preserving full reproducibility from the deterministic generator. **(2) Goldens RELOCATED out of the git-ignored trees into `conformance/goldens/<fixture>.<verb>.json`.** The seven goldens previously lived *inside* the git-ignored fixture trees (e.g. `valid/minimal/validate.golden.json`); the Rust tests now read them from `conformance/goldens/` (path flattened `/`→`-`, e.g. `goldens/invalid-non-monotonic-time.validate.json`). *Why:* `regenerate.sh` → `build.py` → `mutate._copy_baseline` `rmtree`s each invalid target tree and rewrites the valid baseline in place, so a golden living **under** those trees was **deleted on every regenerate** (the generator's `ignore_patterns("*.golden.json")` hack protected only the baseline *copy*, never the pre-existing target contents `rmtree` wiped). Moving the goldens to a directory the generator never touches makes **"a clean `regenerate.sh` then `cargo test` stays green with all goldens intact"** an invariant rather than a race. `hdx-core` was **frozen** for all of MS8 (no reader/rule/domain-type/manifest-floor change); the milestone added only fixtures + tests + goldens + docs. | Records the maintainer decision that fixture data is reproducible-not-tracked and the lesson that goldens MUST live outside the trees `regenerate.sh` wipes — otherwise every regenerate silently deletes the committed assertion baselines, breaking the suite's central acceptance gate (regenerate → test green). Lets a future agent understand why `conformance/goldens/` exists as a separate tracked dir and why no fixture bytes appear in git. |
| 2026-06-02 | **MS4-S1 decisions frozen (types-first, before any reader).** Six load-bearing decisions, recorded here in one reviewable place so S2–S4 are coded against a settled contract. **(1) R1 (Zarr/COG/geometry) decided — pure-Rust, no GDAL.** Zarr via the chosen pure-Rust Zarr-v3 metadata crate (filled in at S2), COG via the pure-Rust `tiff` crate + manual GeoKey/tag parsing (filled in at S3), geoparquet via the **already-present** `arrow`/`parquet` stack reused for its schema + `basin_id`/`delineation` columns + `geo` KV (S4 — no new crate). **LOW contingency:** if a pinned crate's API differs at implementation time (it cannot be confirmed offline), pin the working adjacent major and append a one-line follow-up amendment here — a version surprise is a recorded pin-bump, never an ad-hoc red commit. This covers the Zarr crate, the `tiff` crate, and any decompressor. **(2) The single `GridExtent` convention.** NW cell-EDGE origin + per-axis signed `GridResolution` (GeoTIFF-native). The COG reader takes the affine tiepoint verbatim (already edge-based); the Zarr reader converts its cell-CENTER `lat`/`lon` arrays to edges with the half-pixel rule `west = lon[0] − x_res/2`, `north = lat[0] − y_res/2` (signs per axis), via `grid::center_to_edge`. Verified on the MS2 fixture: Zarr centers `lon[0]=10.125`/`lat[0]=49.875`, res `0.25` → edges `10.0`/`50.0` == the COG tiepoint. *Why:* two genuinely-aligned artifacts MUST yield identical extents for the **G2** alignment precondition (observed in S5, enforced MS6) to be observable — this is the fix for the prior structural-misread HIGH defect. **(3) The CRS-recording rule.** A reader records `Crs` as a comparable `EPSG:<code>` string whenever an EPSG authority/code resolves (Zarr `spatial_ref`/`crs_wkt`→EPSG; COG `GeoKeyDirectory`→EPSG; geoparquet PROJJSON `id.authority=="EPSG"`→`EPSG:<code>`); when no EPSG id resolves it records the raw CRS string verbatim and flags that file's M5-readiness as an **R3** item (documented, never silently claimed). Seeds MS6 **M5**; MS4 records, never cross-checks. **(4) MED-4 — COG band-description three-outcome protocol.** A *named* protocol S3 executes against the fixture round-trip (outcome filled in when S3 lands): (1) pure-Rust read works — the `tiff` crate surfaces tag **42112 GDAL_METADATA** (ASCII) and HDX parses the small fixed `<GDALMetadata>` XML for `<Item … role="description">`(= field name) + `<Item name="units" …>`(= units); G1 COG-side is metadata-deep and live (ground truth: the fixture stores the band name in tag 42112, **not** IFD tag 270); (2) pure-Rust fails, GDAL accepted — record the GDAL system-dependency cost as an explicit amendment **and** confirm the MS9 maturin/PyO3 wheel still builds (never silently reintroduced); (3) pure-Rust fails, GDAL rejected — G1 COG band-name verification is an **R3** byte/format-deep SKIP-with-reason, never silently claimed. **Mismatch rule:** if the chosen reader cannot read the band descriptions the MS2 generator wrote, that is an **MS2 regenerate** (write the descriptions in a tag the reader supports), **never** a reader workaround. **(5) MED-5 — §8 consolidated-metadata gate.** S2 MUST confirm **from the Rust side** that the fixture's Zarr v3 store exposes its metadata via the §8 consolidated path (one read to learn the store): either the reader reads it via the consolidated path (**live**), OR consolidated-metadata / v3-sharding verification is classified as an **R3** byte-deep SKIP with a stated reason (documented, never silently claimed). A zarr-python-vs-Rust mismatch is fixed by **regenerating the fixture**, never a reader workaround (ground truth: `consolidated_metadata.kind == "inline"` in the root `zarr.json`, all six members present). **(6) LOW-3 — no-gridded-chunk/no-pixel review gate.** No gridded-chunk decode happens anywhere in `hdx-core`: the gridded readers read only Zarr array metadata + 1-D `lat`/`lon`/`time` arrays + CF `grid_mapping`, and COG tags/band metadata/georef — **never** `c/` chunk payloads or pixel rasters; the `gridded_*` subtrees are opaque leaves to the layout walk and metadata-only to the readers. S1 lands the `grid` value types (`GridExtent`/`GridResolution`/`GridInfo` + `center_to_edge`, all inert/agnostic, fields private with getters) and six named-field error variants (`ZarrRead`, `CogRead`, `GeoparquetRead`, `MissingGridGeoref`, `MissingGriddedCoordinate`, `MissingGeometryColumn`); a gridded field is an ordinary MS1 `Field` (gridded quadrant + `Some(GridLabel)`), **no** new field type. | Freezes the two load-bearing decisions (the single edge convention + the R1 crate choices) and the three protocol gates (MED-4/MED-5/LOW-3) **before** any reader is written, so S2–S4 build against a settled contract; the half-pixel convention — the fix for the prior structural-misread defect — is reviewable and unit-pinned (`10.125→10.0`, `49.875→50.0`) in one place (spec §1/§2/§7/§8; §14 G1/G2/G3/Geo1/M5 foundations, enforcement MS6). |
| 2026-06-02 | **R1-parquet decided (MS3-S1): pure-Rust `arrow`/`parquet`, no GDAL.** Added `arrow = "58"` (`default-features = false`) and `parquet = "58"` (`default-features = false`, feature `arrow`) to `crates/core` (plus `bytes = "1"` for the in-memory `ChunkReader`), pinned to major **58**. Features are minimal: parquet metadata + 1-D column reads from the **local filesystem only** — no `async`, no `object_store`, no extra codecs. This resolves the **parquet half** of R1 (§7); the **Zarr/COG/geoparquet** half of R1 is **deferred to MS4**. Landed with a private `parquet_meta` touchpoint (open bytes → arrow schema + row-group count) and unit tests over an in-test parquet buffer, so the choice is exercised, not just declared. | Settles §7 R1 for the scalar physical encoding (§4/§8) early so the rest of MS3 builds on a proven, GDAL-free metadata reader (§1: read metadata, not chunks). Avoids the GDAL system dependency; the pure-Rust stack is mature for parquet. |
| 2026-06-02 | Initial architecture authored (STEP 1). | Baseline; no amendments yet. |
