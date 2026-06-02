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
| 2026-06-02 | Initial architecture authored (STEP 1). | Baseline; no amendments yet. |
