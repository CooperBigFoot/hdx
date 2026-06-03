# HDX — Hydrology Dataset Exchange

HDX is a prescriptive, cloud-optimized **data interface** for per-basin hydrology datasets. It specifies *what the bytes look like and how they are organized* — nothing more. Its primary purpose is cloud training of deep-learning models over random `(basin, time-window, fields)` access; the same files may equally be hosted on a local filesystem.

> **The governing discipline (load-bearing): HDX describes the *shape* of data, never *what was done to it*.**

HDX is **inert** and **agnostic**. A conformant reader/writer carries, requires, and interprets **none** of the following: transform / normalization params (μ/σ, log-ε), field **roles** (target / forcing / future-known), **semantic types** (continuous / categorical), the gridded→lumped **reduction**, or any **provenance of computation**. HDX does not know whether a dataset is raw forcing or model output — **a prediction dataset is just an HDX dataset**, validated by the same rules. Field names are opaque producer strings; HDX parses none of them.

This repository is the **contract executed**: a pure-Rust core (`hdx-core`) implementing the two contract-executing verbs `validate` and `describe`, a thin JSON-emitting CLI over them, a PyO3 Python binding, and a conformance fixture suite (a deterministic generator + tracked goldens). The canonical contract is [`spec/HDX_SPEC.md`](spec/HDX_SPEC.md); the living build doc is [`architecture.md`](architecture.md).

## The field — the spine of HDX

The unit of HDX is the **field** (a scientific variable, a QC mask, a cluster id, and a model prediction are *all just fields*; HDX privileges none). A field has two independent axes — **temporal** (`static` | `dynamic`) and **shape** (`scalar` | `gridded`) — yielding four quadrants:

| Quadrant | Per-basin shape | Example | Physical encoding |
|---|---|---|---|
| `scalar · static`  | `[]`      | drainage area     | parquet column (dataset-level rollup) |
| `scalar · dynamic` | `[T]`     | outlet streamflow | parquet column (per-basin) |
| `gridded · static` | `[Y,X]`   | elevation raster  | COG band (per-basin) |
| `gridded · dynamic`| `[T,Y,X]` | precip over grid  | Zarr v3 array / variable (per-basin) |

The quadrant is a **per-field** classification, never a dataset-level mode: a single dataset's schema may freely mix all four. The `shape` axis is deliberately `scalar` vs `gridded`, *not* "lumped vs gridded" — "lumped" smuggles in a reduction, whereas a scalar value (outlet streamflow at a gauge) is often scalar by nature. Field name → column / CF variable / COG band is 1:1 and opaque; companion masks (`{field}_was_filled`) and `{source}_{variable}` names are recognized as **ordinary fields with no special handling**.

## On-disk layout — basin-first hive

The directory structure *is* the contract; only the file format changes across the 2×2. Partitioning is **basin-first** (natural access: "give me everything for basin X").

```
<hdx-dataset>/
  manifest.json                       # the irreducible floor (exactly six fields)
  scalar_static.parquet               # ROOT rollup; 1 row/basin; cols = basin_id + static scalar fields
  outlines.geoparquet                 # ROOT rollup; rows = (basin_id, delineation, geometry)
  basin=<id>/
    scalar_dynamic.parquet            # rows = real `time` axis; cols = basin_id + dynamic scalar fields
    gridded_static/<grid-label>.tif   # multiband COG; band description = field name
    gridded_dynamic/<grid-label>.zarr # Zarr v3; CF variable = field name; CF lat/lon + grid_mapping
  basin=<id>/ …
```

The asymmetry is principled (it tracks data size/shape, not convention): the two small dataset-level rollups sit at the root; only the large per-basin data lives under `basin=<id>/`. A dataset carries whatever subset of the four physical encodings its field schema implies — a scalar-only dataset has no `gridded_*` artifacts.

Key invariants enforced as conformance:

- **Homogeneity** — every basin has the *identical field schema*. A field absent for a basin is **present-but-NaN, never a missing file**. Discovery is therefore a one-basin read.
- **Time** — a real temporal type (parquet `Timestamp`, Zarr CF integer-since-epoch); the `String "YYYY-MM-DD"` hack is forbidden. The `time` column is named `time`, non-nullable, sorted ascending. Within a basin, `scalar_dynamic` and every `gridded_dynamic` artifact share one identical axis (gaps NaN-filled); across basins, periods of record may be ragged.
- **Grids** — per-variable native grids (no imposed common grid), one dataset-wide CRS (recommend EPSG:4326). One artifact = one grid; a grid label shared across the `gridded_static` and `gridded_dynamic` subtrees signals cell-for-cell alignment.
- **Geometry** — outlines ship *in* HDX, plural: one row per `delineation` (MERIT, GRIT, HydroBASINS, a custom or hand-drawn polygon — a neutral label, not a trusted "hydrofabric"), in one dataset-level non-partitioned `outlines.geoparquet`.

## The manifest — the irreducible floor

Hive partitioning + self-describing files + homogeneity make almost everything discoverable, so the manifest declares only what is **not derivable**. It is **exactly six fields**:

```json
{
  "format_version": "0.1",
  "name": "<dataset name>",
  "created_at": "2026-06-01T00:00:00Z",
  "producer_version": "<tool/version that wrote it>",
  "crs": "EPSG:4326",
  "cadence": "daily"
}
```

Adding any derivable field (a content hash, a data version, a field catalog, a basin list) is a **conformance bug**, made unrepresentable by the parser (`deny_unknown_fields`). `format_version` is read **first** and is a **hard version cut**: only `"0.1"` is accepted (exact-string — `"0.10"` ≠ `"0.1"`), any other value is rejected outright. There are no multi-version readers; HDX versions the contract, not the content.

## What this repository provides

### `hdx-core` — the two verbs (the spec executed)

All contract logic lives in [`crates/core`](crates/core/) (see [`crates/core/README.md`](crates/core/README.md) for the full module map and glossary). It reads **metadata, not gridded chunks** — parquet footers/schemas, Zarr v3 consolidated metadata + 1-D `lat`/`lon`/`time` coordinate arrays, COG/GeoTIFF tags + band descriptions, geoparquet schema + 1-D `delineation`/`basin_id` columns. The stack is **pure Rust, with no GDAL and no C toolchain** (`arrow`/`parquet`, `zarrs_metadata` + `ruzstd`, `tiff`).

- `describe(path) -> Result<Description, _>` / `describe_json(path) -> Result<String, _>` — **discovery**. Emits the full self-description discovered from the files (field catalog with quadrant per field, per-grid extent/resolution/CRS, per-basin ragged time extents, units, delineation labels, basin list) as JSON. Facts only — no conformance verdict. `describe` is the spec's declared stress test of the manifest floor.
- `validate(path) -> Result<ValidationReport, _>` / `validate_json(path) -> Result<String, _>` — **conformance**. Runs the spec §14 `MUST` checklist (the 20 ids `M1`–`M6`, `L1`–`L3`, `I1`–`I3`, `H1`–`H2`, `T1`–`T2`, `G1`–`G3`, `Geo1`) over the same shared discovery layer and emits a `ValidationReport` of per-check outcomes — each recording **ran vs skipped**, **pass/fail**, its metadata-deep/byte-deep depth, and a detail/reason — plus an overall `conformant: bool`. It **fails closed** (a violated `MUST` that ran ⇒ non-conformant; a skip never flips the verdict) and honestly reports which checks ran. Byte-deep legs not yet implemented in v0.1 (`L3`, the `M6` axis-regularity leg, `T2`) are reported `skipped` with a reason.

Both verbs perform the §0 hard version cut and the six-field manifest boundary-parse **before** any discovery. A `conformant: false` verdict is a recorded report outcome, distinct from a returned error (reserved for structural/entry failures), so the CLI can map the two to different exit codes.

### The `hdx` CLI

A thin, JSON-emitting, LLM-drivable binary (root package, [`src/main.rs`](src/main.rs)) wraps the two verbs — arg-parse → call `hdx-core` → serialize result to stdout → exit code. JSON is *output* (stdout); diagnostics go through `tracing` to stderr. The v0.1 surface:

```sh
hdx describe ./my-dataset    # prints the Description JSON to stdout
hdx validate ./my-dataset    # prints the ValidationReport JSON to stdout
```

| Exit code | Meaning |
|---|---|
| `0` | success — `describe` succeeded, or `validate` returned `conformant: true` |
| `1` | non-conformant — `validate` returned `conformant: false` |
| `2` | usage / IO error — bad args, unreadable path, unknown `format_version` (hard cut), malformed manifest |

### Conformance fixtures

[`conformance/`](conformance/) holds a **dev-only Python fixture generator** (never shipped in `hdx-core`, not an HDX writer) plus the golden outputs (see [`conformance/README.md`](conformance/README.md)). The fixture **data** trees are **git-ignored and regenerated** from the deterministic generator (run `conformance/generator/regenerate.sh` before `cargo test`); only the generator source and the small goldens are tracked.

- `valid/minimal/` — one valid three-basin, four-quadrant dataset (shared aligned `era5` grid label, ragged-across/aligned-within time axes, companion-mask + `{source}_{variable}` fields, plural delineations).
- `valid/irregular-time-axis/` — a second valid-shaped dataset that stays `conformant: true` while `M6`'s axis-regularity leg is honestly reported *skipped* (the no-enforceable-M6-negative finding).
- **12 fail-closed invalids**, each one surgical mutation off the baseline, each pinning exactly one §14 check (`M2`–`M6`, `L1`/`L2`, `I1`/`I2`, `H1`/`H2`, `T1`, `G2`). The full classification of all 20 ids — including the checks with no isolable on-disk negative in v0.1 (`I3`, `G1`, `G3`, `Geo1`) and the byte-deep skips (`L3`, `T2`, `M6`-regularity) — lives in `conformance/README.md`.
- Golden `describe`/`validate` outputs in [`conformance/goldens/`](conformance/goldens/) (produced by `hdx-core`, kept *outside* the regenerated trees), pinned by snapshot tests and validated against the JSON Schemas in [`schemas/`](schemas/) (`manifest.schema.json`, `describe.schema.json`, `validate.schema.json`).

### The Python binding

[`crates/python/`](crates/python/) is a **PyO3 binding** (abi3, built with `maturin`) that **mirrors** `validate`/`describe` over the same `hdx-core` API — it adds no contract logic and re-derives no wire shape. `hdx.describe(path)` / `hdx.validate(path)` return the same JSON-shaped `dict`s; the §0 hard version cut surfaces as an `hdx.UnknownFormatVersionError` (an `hdx.HdxError`). See [`crates/python/README.md`](crates/python/README.md).

### Explicitly out of scope

`regrid`, `clip`, and `reduce` are **excluded from HDX entirely** — they encode hydrology (area-weighting, partial-cell handling, resampling kernels), not the contract, and belong to a separate data-operations engine. They MUST NOT appear in `hdx-core`.

## Build & development quickstart

```sh
cargo build
# conformance fixture data is git-ignored — regenerate it before testing:
PYTHON=python3.12 conformance/generator/regenerate.sh
cargo test
cargo clippy --all-targets -- -D warnings
cargo run -- describe conformance/valid/minimal     # Description JSON to stdout

# Python binding (mirrors the verbs):
crates/python/run_python_tests.sh                    # maturin build + pytest over regenerated fixtures
```

Project conventions (see [`CLAUDE.md`](CLAUDE.md) / [`AGENTS.md`](AGENTS.md)):

- **Edition 2024.** Workspace root is the `hdx` binary; `crates/*` are members.
- **Mandatory per-commit version bump + tag.** Every commit runs `./scripts/bump-version.sh patch`, stages `Cargo.toml`, commits with a conventional message, then `git tag v<version>`. Use `minor`/`major` only when explicitly requested.
- **Logging via `tracing` only** — never `println!` for diagnostics. The CLI emits JSON to stdout (output); diagnostics go to stderr.
- **Library code** (`crates/`) uses `thiserror` and never `.unwrap()`/`.expect()`; **application glue** (`src/`) uses `anyhow` with `.context()`.
- **Parse, don't validate**; enums over booleans; newtypes for confusion-prone values. Invalid states are unrepresentable downstream of the boundary.

## Repository layout

| Path | Contents |
|---|---|
| [`spec/HDX_SPEC.md`](spec/HDX_SPEC.md) | The canonical, normative contract (source of truth for *what HDX is*). |
| [`architecture.md`](architecture.md) | The living build-architecture doc (crate layout, type model, milestone hints, amendments log). |
| [`crates/core/`](crates/core/) | `hdx-core` — all contract logic: type model, format readers, `validate`, `describe`. |
| `src/main.rs` | The thin `hdx` CLI (root binary). |
| [`schemas/`](schemas/) | JSON Schemas for `manifest.json`, the `describe` output, and the `validate` report. |
| [`conformance/`](conformance/) | Dev-only fixture generator + valid/invalid fixtures + golden outputs. |

## Status

**HDX v0.1 (`format_version = "0.1"`) is feature-complete.** The shared discovery layer and both verbs (`validate` over the full 20-check §14 `MUST` set, and `describe`) are implemented in `hdx-core` (pure-Rust readers, no GDAL); the thin `hdx` CLI wraps them with the `0`/`1`/`2` exit-code contract; the conformance suite covers every §14 id (regenerated fixtures + tracked goldens); and the PyO3 binding in `crates/python` mirrors the verbs into Python. Built milestone-by-milestone with an adversarial plan→critique→execute workflow; the durable build decisions and amendments live in [`architecture.md`](architecture.md) (the full per-milestone plan/critique trail is in git history, not the tracked tree).
