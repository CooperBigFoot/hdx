# MS2 — Fixture generator: one valid + two minimal invalid datasets — STEP PLAN

> **Milestone:** MS2 (resolves R2, part 1 — the fixture problem, before any reader exists).
> **Source contract:** `spec/HDX_SPEC.md` (canonical, settled).
> **Planned against:** `architecture.md` §1/§2/§4/§7 (R2) and `planning/milestones.md`
> (MS2 goal, deliverables, reviewable outcome, exit criteria, spec refs, risks).
> **Folded critique (STEP-2):** **MED-5** (writer/reader self-assertion linkage —
> the engineered properties most at risk of writer/reader mismatch, parquet `time`
> row-group statistics and Zarr consolidated metadata, MUST be confirmed from the
> RUST side in MS3/MS4, and any mismatch is fixed by REGENERATING the fixture, never
> a reader workaround; named hand-off) and **LOW-2** (derived, not hand-authored —
> every invalid fixture, and the larger MS8 family later, MUST be generated
> programmatically from the single valid baseline via exactly one surgical mutation
> each; a hard rule recorded in `conformance/README.md`).
>
> **Why this milestone is MS2.** MS1 (the Rust type model + manifest parser +
> manifest JSON Schema) is already built and green (`crates/core/src/{newtypes,
> error,format_version,field,manifest}.rs`, `schemas/manifest.schema.json`). MS2 is
> the next milestone: it resolves **R2** — there is **no HDX writer in v0.1**, yet
> MS3/MS4 readers and MS6 `validate` need real on-disk parquet / Zarr / COG /
> geoparquet bytes to test against. MS2 stands up a **dev-only, checked-in Python
> fixture generator** that emits one valid + two minimal invalid datasets. It ships
> **no Rust**; its dependency on MS1 is **shape-only** (the frozen six-field manifest
> JSON of §11 and the field/quadrant model of §2), not a Rust build/link dependency.

---

## Scope guard

Every step below stays strictly inside MS2 (milestones.md MS2 / architecture §7 R2):

- **No Rust source changes.** MS2 ships **zero Rust**. The generator is Python and
  links/compiles nothing from `hdx-core`. No reader (`parquet`/`arrow`/`zarrs`/
  `tiff`/`geoarrow`) is added to any `Cargo.toml`. No verb (`describe` MS5,
  `validate` MS6), no §14 rule engine, no CLI (MS7) — those are later milestones.
  The only repo files MS2 touches are under `conformance/` (plus the bump+tag of
  `Cargo.toml` mandated by CLAUDE.md, and an optional `.gitignore`/path note).
  "Repo stays green" therefore means: **the unchanged Rust crate still passes
  `cargo build` + `cargo test` + `cargo clippy --all-targets -- -D warnings`** after
  each step (re-run as the gate), while the per-step *meaningful* progress is the
  generator and its self-assertions.
- **The generator is a TEST FIXTURE TOOL, not an HDX writer.** It lives only in
  `conformance/generator/`, is never shipped in `hdx-core`, and is never imported by
  production code. This is stated explicitly in `conformance/README.md` (milestones.md
  MS2: "Generator masquerading as a writer" risk). MS2 does **not** create the
  inverse of any reader; it emits bytes a reader will later read.
- **No enforcement, only seeding.** MS2 enforces **no** spec check. It engineers the
  on-disk *preconditions* for spec checks (L1, L2, L3, I1, I2, I3, H1, H2, T1, T2,
  G1, G2, G3, Geo1, M5, M6) so that MS3–MS6 can read and enforce them later. The
  generator's own checks are **writer-side self-assertions** (Python), distinct from
  Rust-side enforcement — see the MED-5 hand-off below.
- **No later-milestone work.** No regrid/clip/reduce/reduction/hydrology anywhere
  (excluded forever, spec §10). The gridded fields ship **delineation-neutral over
  the bbox** — never pre-clipped or NaN'd to an outline (spec §9). No exhaustive
  one-per-check invalid family (that is MS8); MS2 ships exactly **two** minimal
  invalids.
- **Inert/agnostic discipline (hard rule, spec §1/§13).** `manifest.json` is
  **exactly** the six floor fields (§11) — no content hash, no data-version, no field
  catalog, no basin list, no transform/role/semantic/provenance key. Field names are
  opaque producer strings; the companion-mask (`{field}_was_filled`) and
  `{source}_{variable}` patterns are present **only to prove later milestones give
  them no special handling** — the generator attaches no "belongs-to" link, no role,
  no magic. `delineation` is a neutral label, never a "hydrofabric" source.
- **`format_version` is a HARD cut.** The valid fixture's manifest declares
  `"0.1"`; the `wrong-format-version` invalid declares `"0.2"` and is otherwise
  byte-equivalent (one surgical mutation, LOW-2).

No step performs a later milestone's work, and none violates the inert/agnostic
discipline.

---

## The MED-5 writer/reader self-assertion hand-off (named, not an afterthought)

MS2's self-assertions are **Python-side** and assert what the **writer intended** —
they cannot prove what a **Rust reader** can recover from the same bytes. Two
engineered properties are most at risk of a writer/reader mismatch:

1. **Parquet `time` row-group statistics** — pyarrow may or may not emit usable
   min/max stats for the timestamp logical type with the chosen settings. MS2
   self-asserts the *written file* carries them; **MS3 MUST confirm from the Rust
   side** (`arrow`/`parquet`) that the time extent is sourced from those statistics
   (not the bounded-scan fallback) on the MS2 valid fixture.
2. **Zarr v3 consolidated metadata** — `zarr-python`'s v3 consolidated-metadata
   layout must be readable by Rust `zarrs`. MS2 self-asserts consolidated metadata
   is present; **MS4 MUST confirm from the Rust side** that it reads the store's
   metadata via the §8 consolidated path (or explicitly classify it an R3 byte-deep
   skip with a reason).

**The hand-off rule (folds MED-5):** if MS3/MS4 find that the Rust reader cannot
recover a property the MS2 writer asserted, the fix is to **REGENERATE the fixture**
(adjust the generator and re-emit), **never** to add a reader workaround. This is
stated in the generator source (a header comment on each at-risk write) **and** in
`conformance/README.md` as a named MS3/MS4 hand-off, so a future agent treats a
mismatch as a generator bug, not a reader bug.

---

## The LOW-2 derived-not-hand-authored hard rule

Both invalid fixtures (and the full MS8 family later) MUST be **generated
programmatically from the single valid baseline via exactly one surgical mutation
each** — never hand-edited trees. The generator builds the valid baseline once, then
derives each invalid by applying one targeted mutation (e.g. overwrite
`manifest.json`'s `format_version`; delete one root rollup). `conformance/README.md`
records this as a **hard rule**: a later contributor **MUST NOT** hand-edit a fixture
tree; they MUST add a mutation to the generator and regenerate. This keeps every
fixture one mutation off a known-good baseline, so "differs in exactly one way" is
true by construction and maintainable as one generator, not N hand trees.

---

## Ordering rationale

The steps follow build-tractability and dependency order; each is one conventional
commit and leaves the **Rust** repo green (the unchanged crate still builds/tests/
clippies clean), with the generator advancing meaningfully:

1. **S1 — generator project skeleton + pinned deps + dev-only declaration first.**
   Everything downstream runs inside this Python project. It pins every dependency
   (reproducibility — milestones.md MS2 top risk), provides the `regenerate.sh`
   entry point (initially a no-op stub that exits 0), declares the **dev-only /
   not-a-writer** rule and the **LOW-2** + **MED-5** rules in a first
   `conformance/README.md`, and adds a `.gitignore` note for any scratch. No
   fixtures yet — this is the harness. Committable and green (Rust untouched).
2. **S2 — the valid baseline: scalar half (manifest + scalar_static + per-basin
   scalar_dynamic + outlines).** The simplest, mature-format half (parquet /
   geoparquet via pyarrow). Establishes ≥2 basins, the six-field manifest, in-file
   `basin_id` == folder + uniqueness, the `time` column (full timestamp,
   non-nullable, sorted) **with row-group statistics**, ragged-across-basins time,
   plural `outlines` with `(basin_id, delineation, geometry)`. Self-asserts each
   scalar-side engineered property. This is the spine the gridded half aligns to.
3. **S3 — the valid baseline: gridded half (COG + Zarr) sharing one aligned grid
   label.** Adds the `gridded·static` COG and `gridded·dynamic` Zarr per basin,
   engineered so both share **one** grid label and are **cell-for-cell aligned**
   (the G2 positive-path precondition), with CF georef (Zarr lat/lon + grid_mapping)
   and GeoTIFF georef tags, the Zarr time axis **identical** to the basin's scalar
   `time` (T2 precondition), Zarr written with **consolidated metadata + v3
   sharding**, and the `{source}_{variable}`/companion-mask field-naming patterns
   present. Completes the four-quadrant valid dataset. Self-asserts shared-label
   alignment, intra-basin time alignment, consolidated-metadata presence. Placed
   after S2 so the Zarr can align to the already-written scalar time axis.
4. **S4 — derive the two minimal invalids by one surgical mutation each (LOW-2).**
   With the valid baseline complete, derive `invalid/wrong-format-version/` (mutate
   manifest `format_version` → `"0.2"`; pins M2) and `invalid/missing-root-rollup/`
   (delete one root rollup; pins L1), each programmatically from the baseline. A
   self-assertion confirms each invalid differs from the baseline in exactly the one
   intended way. Placed last among generation steps: it depends on the full baseline.
5. **S5 — finalize `conformance/README.md`: layout docs + check-id table + the
   MED-5 hand-off + the LOW-2 hard rule, and wire the full `regenerate.sh`.** Pure
   documentation + the end-to-end regenerate target that produces all three trees
   deterministically and runs every self-assertion (aborting on failure). Placed
   last so the README documents the final, complete fixture set.

Each step is one conventional commit, ends with `./scripts/bump-version.sh patch` +
stage `Cargo.toml` + commit + `git tag v<version>` (CLAUDE.md / architecture §2), and
after it `cargo build` + `cargo test` + `cargo clippy --all-targets -- -D warnings`
all pass on the unchanged Rust crate.

---

## Steps

### MS2-S1 — Generator project skeleton, pinned deps, dev-only + LOW-2 + MED-5 rules

**Intent.** Stand up the dev-only Python fixture-generation harness that every later
step runs inside: a pinned, reproducible Python project under
`conformance/generator/`, a `regenerate.sh` entry point (initially a stub that exits
0 with a "no fixtures yet" message via the generator's logger, **not** `print` for
diagnostics where avoidable), and a first `conformance/README.md` that records the
three load-bearing rules — **generator is dev-only and is not an HDX writer**,
**LOW-2** (derived-not-hand-authored), and the **MED-5** MS3/MS4 confirmation
hand-off — before any fixture exists. Independently committable: it adds only files
under `conformance/`, changes no Rust, and leaves the crate green.

**Changes.**
- `conformance/generator/pyproject.toml` (or `requirements.txt` + `requirements.lock`)
  — pins every dependency to an exact version: `pyarrow` (parquet + geoparquet),
  `xarray` + `zarr` (Zarr **v3**), `rioxarray`/`rasterio` (COG), `geopandas` +
  `shapely` (geometry), `numpy`/`pandas` as needed. Pin a Python version known to be
  compatible with the chosen `zarr` v3 release (see Risks — the host Python is 3.14,
  which some pinned `zarr`/`numpy` wheels do not yet support; the generator MUST
  declare and, if needed, create its own venv with a compatible interpreter).
- `conformance/generator/regenerate.sh` — executable; for S1 a **stub** that sets up
  the venv (idempotently), prints a single "MS2-S1: harness only, no fixtures yet"
  line, and exits 0. (A `Makefile` target `regenerate` MAY wrap it.)
- `conformance/generator/hdx_fixtures/` (package dir) with `__init__.py` and a
  `logging`-configured module-level logger (diagnostics go through `logging`, not raw
  `print`; the script's user-facing status line is output, akin to the CLI/JSON
  distinction in architecture §2).
- `conformance/README.md` — first version: states (1) the generator is **dev-only**,
  lives only in `conformance/`, is **never shipped in `hdx-core`** and is **not an
  HDX writer** (milestones.md MS2 risk); (2) the **LOW-2 hard rule** — every invalid
  fixture is derived programmatically from the single valid baseline via exactly one
  surgical mutation; **no hand-editing fixture trees**; (3) the **MED-5 hand-off** —
  parquet `time` row-group statistics and Zarr consolidated metadata are
  writer-asserted here but MUST be confirmed from the Rust side in MS3/MS4, and any
  mismatch is fixed by **regenerating** the fixture, never a reader workaround.
- `.gitignore` — add the generator's venv / `__pycache__` / scratch paths so only
  source + emitted fixtures are committed (no Rust impact).

**Test plan.**
- Run `conformance/generator/regenerate.sh`: it sets up the venv, imports every
  pinned dependency successfully (a smoke import in the stub proves the pins resolve
  on the chosen interpreter), prints the status line, and exits 0.
- Confirm `conformance/README.md` contains the three rules (dev-only/not-a-writer,
  LOW-2, MED-5 hand-off).
- **Rust gate (the green check):** `cargo build`, `cargo test`,
  `cargo clippy --all-targets -- -D warnings` all pass on the unchanged crate.

**Acceptance.**
- `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings` all pass
  (Rust unchanged).
- `regenerate.sh` runs end-to-end (stub), the pinned deps import on the declared
  interpreter, and the run is reproducible.
- `conformance/README.md` records the **dev-only/not-a-writer** rule, the **LOW-2**
  hard rule, and the **MED-5** MS3/MS4 confirmation hand-off (folds both critique
  issues at the doc level before any fixture exists).
- No Rust source change; no reader crate added; inert/agnostic discipline intact (no
  fixture, nothing to violate yet).
- Patch-bump + stage `Cargo.toml` + conventional commit + `git tag v<version>`.

**Spec refs.** §10 / architecture §7 R2 (no writer; dev-only generator),
architecture §2 (placement: nothing outside `hdx-core` implements contract logic;
diagnostics vs output), milestones.md MS2 (generator project, pinned deps,
regenerate script, dev-only rule).

**Commit message.** `chore(conformance): scaffold dev-only fixture generator harness`

---

### MS2-S2 — Valid baseline, scalar half: manifest + scalar_static + scalar_dynamic + outlines

**Intent.** Build the scalar/geometry half of the one valid dataset on the mature
parquet/geoparquet path: the six-field `manifest.json`, the root
`scalar_static.parquet` rollup (1 row/basin), per-basin `scalar_dynamic.parquet`, and
the root `outlines.geoparquet` — for **≥2 basins** with **ragged-across-basins** time
extents. This establishes the spine (basins, identity, the shared `time` axis) the
gridded half (S3) aligns to. Independently committable: it emits a partial-but-valid
scalar tree plus self-assertions; the Rust crate is untouched and green. (The dataset
is not yet four-quadrant — S3 adds the gridded fields — but every artifact this step
emits is conformant on its own terms.)

**Changes.**
- `conformance/generator/hdx_fixtures/manifest.py` — emits `manifest.json` with
  **exactly** the six §11 floor fields (`format_version: "0.1"`, `name`,
  `created_at` RFC 3339 `Z`-form, `producer_version`, `crs: "EPSG:4326"`,
  `cadence: "daily"`). No seventh/derivable key (inert/agnostic, §11). The JSON shape
  matches MS1's `schemas/manifest.schema.json` (shape-only dependency on MS1).
- `conformance/generator/hdx_fixtures/scalar.py` — writes:
  - `scalar_static.parquet` at the dataset root: one row per basin, columns
    `basin_id` + at least one `scalar·static` field (e.g. `drainage_area`, f64), with
    zstd compression.
  - per-basin `basin=<id>/scalar_dynamic.parquet`: rows = `time`, columns `basin_id`
    + at least one `scalar·dynamic` field (e.g. `streamflow`, f64). The `time` column
    is a **full timestamp** (date+time), **non-nullable**, **sorted ascending**
    (spec §6 / T1), written **with row-group statistics on `time`** (spec §8), zstd.
  - **Ragged across basins** (basins differ in period of record, §6.1) while each
    basin's own axis is internally consistent (the within-basin alignment to the
    gridded Zarr is completed in S3, §6.2).
  - In-file `basin_id` **equals** the `basin=<id>` folder for every basin, and
    `basin_id` values are **unique** across basins (I2/I3 preconditions).
- `conformance/generator/hdx_fixtures/outlines.py` — writes `outlines.geoparquet` at
  the root with rows `(basin_id, delineation, geometry)`, **≥2 `delineation` labels**
  for at least one basin (§9 plurality), **not partitioned** by delineation (a single
  file at the root), neutral `delineation` labels (never a "hydrofabric" source).
- `conformance/generator/regenerate.sh` — extended to invoke the scalar/outlines
  emit into `conformance/valid/minimal/` and run the scalar self-assertions.
- `conformance/generator/hdx_fixtures/assertions.py` — scalar-side
  **writer self-assertions** (abort generation on failure):
  - `time` column is full-timestamp, non-nullable, sorted ascending **and** the
    written parquet carries usable **min/max row-group statistics** on `time`
    (re-open with pyarrow and read the file metadata's row-group statistics). A
    **header comment** on this assertion records the **MED-5** rule: this is a
    *writer* assertion; **MS3 confirms recoverability from the Rust side**, and a
    mismatch is fixed by regenerating, not a reader workaround.
  - in-file `basin_id` == `basin=<id>` folder for every basin; `basin_id` unique.
  - time extents are **ragged across basins** (§6.1).
  - `outlines.geoparquet` has exactly `(basin_id, delineation, geometry)`, ≥2
    delineation labels for ≥1 basin, single file (not partitioned).
- `conformance/valid/minimal/` — the committed emitted scalar tree (`manifest.json`,
  `scalar_static.parquet`, `basin=<id>/scalar_dynamic.parquet`, `outlines.geoparquet`).

**Test plan.**
- Run `regenerate.sh`; the scalar self-assertions all pass (a deliberately broken
  variant — e.g. an unsorted `time` — is shown to abort generation, proving the
  assertions are load-bearing; this check is run manually/locally, not committed as a
  broken fixture).
- Inspect with pyarrow: `scalar_static` has `basin_id` + the static field, 1 row/
  basin; each `scalar_dynamic` has `basin_id` + the dynamic field and a sorted,
  non-null full-timestamp `time` column whose file metadata exposes row-group
  min/max statistics; `outlines.geoparquet` schema is `(basin_id, delineation,
  geometry)` with ≥2 labels for ≥1 basin.
- Confirm `manifest.json` is exactly six fields and validates against the committed
  `schemas/manifest.schema.json` (run the existing MS1 schema, e.g. via a quick
  `jsonschema` check in the generator's dev env — proving shape agreement with MS1).
- **Rust gate:** `cargo build`, `cargo test`,
  `cargo clippy --all-targets -- -D warnings` all pass (Rust unchanged).

**Acceptance.**
- `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings` all pass.
- The scalar/geometry artifacts exist under `conformance/valid/minimal/` for ≥2
  basins; all scalar-side self-assertions pass (abort on failure).
- **Seeds (not enforces):** T1 (time type/sort/non-null), I1/I2/I3 (basin_id present/
  folder-agreement/unique), L1 (root rollups present), §6.1 ragged time, §8 `time`
  row-group statistics, Geo1 (`outlines` schema + `delineation` + not partitioned),
  M5 precondition (manifest `crs` present to cross-check files in MS6).
- The **MED-5** writer-side `time`-statistics assertion is present with the header
  comment naming the MS3 Rust-side confirmation hand-off.
- `manifest.json` is exactly the six floor fields (inert/agnostic; §11) and validates
  against the MS1 schema.
- Patch-bump + stage `Cargo.toml` + conventional commit + `git tag v<version>`.

**Spec refs.** §2 (fields; ordinary), §3 (basin_id authoritative, folder agreement,
uniqueness), §4 (basin-first hive; root rollups), §5 (homogeneity — same scalar
schema across basins), §6 / §6.1 (real temporal type; `time` column; ragged across
basins), §8 (parquet sorted by `time` + row-group statistics), §9 (plural outlines,
`delineation` column, not partitioned), §11 (six-field manifest), §10/R2 (dev-only
generator).

**Commit message.** `feat(conformance): generate valid baseline scalar + outlines tree`

---

### MS2-S3 — Valid baseline, gridded half: aligned COG + Zarr sharing one grid label

**Intent.** Complete the one valid dataset into a **four-quadrant** dataset by adding
the `gridded·static` COG and `gridded·dynamic` Zarr per basin, engineered so the two
artifacts **share one grid label** and are **cell-for-cell aligned** (the G2
positive-path precondition), each carries standard self-describing georeferencing,
the Zarr time axis is **identical** to the basin's scalar `time` (the T2 precondition),
the Zarr is written with **consolidated metadata + v3 sharding** (§8), and the
field-naming patterns (`{source}_{variable}` and `{field}_was_filled`) appear so later
milestones can prove they get no special handling. Independently committable: it
extends the same `conformance/valid/minimal/` tree and adds gridded self-assertions;
Rust is untouched and green. Placed after S2 so the Zarr time axis aligns to the
already-written scalar `time`.

**Changes.**
- `conformance/generator/hdx_fixtures/grids.py` — writes, per basin, into the **same**
  `conformance/valid/minimal/basin=<id>/`:
  - `gridded_static/<label>.tif` — a multiband COG (internal tiling + overviews),
    each **band description = field name** (no positional channel axis, G1), standard
    GeoTIFF georeferencing tags (CRS + affine, G3), at least one `gridded·static`
    field (e.g. `elevation`, f32) with units in band metadata. Dense rectangular
    `[Y,X]` over the basin bbox, **delineation-neutral** (not clipped to any outline,
    §9).
  - `gridded_dynamic/<label>.zarr` — Zarr **v3** with each **named CF variable =
    field name** (no positional channel axis, G1), explicit `lat`/`lon` coordinate
    arrays + `grid_mapping`/CRS (CF georef, G3), `time` coordinate as CF
    integer-since-epoch, time-major chunking, **v3 sharding**, **consolidated
    metadata**, blosc-zstd compression. Dense `[T,Y,X]`, delineation-neutral.
  - The COG and the Zarr in each basin use **one and the same `<label>`** (shared
    grid label ⇒ alignment, §8), and are written from the **same affine/extent/
    resolution** so they are **cell-for-cell aligned** (G2 precondition).
  - The Zarr `time` coordinate is the **identical** axis (same timestamps) as that
    basin's `scalar_dynamic.parquet` `time` (T2 precondition, §6.2); gaps a field
    does not natively cover are **NaN-filled** (§6.2).
  - Field naming: at least one field uses `{source}_{variable}` (e.g.
    `era5_precipitation` as the `gridded·dynamic` field) and at least one uses the
    companion-mask `{field}_was_filled` pattern (e.g. a boolean `streamflow_was_filled`
    dynamic field) — present **as ordinary fields**, no role/belongs-to attached
    (spec §2).
- `conformance/generator/hdx_fixtures/assertions.py` — gridded-side
  **writer self-assertions** (abort on failure):
  - per basin, the COG and the Zarr **share the same grid label** and are
    **cell-for-cell aligned** (same CRS, affine, extent, resolution) — the G2
    positive-path precondition.
  - within each basin, the Zarr `time` axis is **identical** to the scalar
    `time` axis (§6.2), and ragged-across-basins still holds (§6.1).
  - the Zarr store carries **consolidated metadata** (re-open and read it) and uses
    **v3 sharding**. A **header comment** records the **MED-5** rule: this is a
    *writer* assertion; **MS4 confirms the Rust `zarrs` reader reads via the §8
    consolidated path (or classifies it an R3 byte-deep skip with a reason)**, and a
    mismatch is fixed by regenerating, not a reader workaround.
  - the `{source}_{variable}` and `{field}_was_filled` fields are present and
    catalogued as ordinary (no suffix/prefix magic in the generator).
  - all four quadrants are now present in the dataset's field set (§2 mix-quadrant).
- `conformance/generator/regenerate.sh` — extended to emit the gridded artifacts and
  run the gridded self-assertions as part of building `conformance/valid/minimal/`.
- `conformance/valid/minimal/basin=<id>/gridded_static/` and `gridded_dynamic/` —
  the committed emitted gridded artifacts.

**Test plan.**
- Run `regenerate.sh`; all gridded self-assertions pass.
- Inspect: COG band descriptions = field names with georef tags; Zarr variables =
  field names with CF lat/lon + grid_mapping; the COG and Zarr in each basin share
  the grid label and are cell-for-cell aligned (compare CRS/affine/extent/res); the
  Zarr `time` matches the scalar `time` for that basin; the Zarr exposes consolidated
  metadata and v3 sharding; the `era5_precipitation` and `*_was_filled` fields appear.
- Confirm the dataset now spans all four quadrants (one each of
  scalar·static / scalar·dynamic / gridded·static / gridded·dynamic).
- **Rust gate:** `cargo build`, `cargo test`,
  `cargo clippy --all-targets -- -D warnings` all pass (Rust unchanged).

**Acceptance.**
- `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings` all pass.
- `conformance/valid/minimal/` is a complete **four-quadrant** valid dataset; all
  gridded self-assertions pass (abort on failure).
- **Seeds (not enforces):** G1 (self-naming variables/bands; no channel axis), G2
  (shared aligned grid label across COG+Zarr — the positive-path precondition), G3
  (CF / GeoTIFF georef), H1/H2 (identical field schema + grid-label set across
  basins), T2 (intra-basin scalar↔Zarr time alignment), §6.1 ragged-across, §6.2
  NaN-filled gaps, §7 (per-variable native grid, dense rectangular over bbox).
- The **MED-5** writer-side consolidated-metadata assertion is present with the
  header comment naming the MS4 Rust-side confirmation hand-off.
- Inert/agnostic confirmed: companion-mask & `{source}_{variable}` fields are
  ordinary; gridded fields are delineation-neutral (not clipped, §9).
- Patch-bump + stage `Cargo.toml` + conventional commit + `git tag v<version>`.

**Spec refs.** §2 (mix-quadrant; ordinary companion-mask & `{source}_{variable}`),
§5 (homogeneity), §6.2 (intra-basin time alignment; NaN-filled gaps), §7
(per-variable native grids; CF / GeoTIFF georef; one dataset-wide CRS), §8 (one
artifact = one grid; self-naming; shared grid label ⇒ alignment; Zarr v3 sharding +
consolidated metadata), §9 (delineation-neutral gridded fields), §10/R2.

**Commit message.** `feat(conformance): generate aligned gridded COG + Zarr for valid baseline`

---

### MS2-S4 — Derive the two minimal invalids by one surgical mutation each (LOW-2)

**Intent.** Produce the two minimal invalid datasets so MS6 can be observed returning
`conformant:false` in its own milestone — each **derived programmatically from the
complete valid baseline via exactly one surgical mutation** (folds **LOW-2**), and
each differing from the baseline in exactly one way that violates exactly one spec
check. Independently committable: it adds two derivation routines + their committed
trees + a "differs in exactly one way" self-assertion; Rust untouched and green.
Placed after S3 because both invalids derive from the full four-quadrant baseline.

**Changes.**
- `conformance/generator/hdx_fixtures/mutate.py` — a small derivation layer that
  **copies** the valid baseline tree, then applies exactly **one** mutation:
  - `invalid/wrong-format-version/` — copy the baseline, then overwrite
    `manifest.json` so `format_version: "0.2"` (everything else byte-identical to the
    baseline manifest). Pins spec-check **M2** (hard cut).
  - `invalid/missing-root-rollup/` — copy the baseline, then **delete** one root
    rollup (`scalar_static.parquet` *or* `outlines.geoparquet`). Pins spec-check
    **L1**. (Document which rollup is removed in the README check-id table.)
- `conformance/generator/hdx_fixtures/assertions.py` — an invalid-side
  **self-assertion**: for each invalid tree, assert it differs from the valid baseline
  in **exactly the one intended way** (a recursive tree diff: the wrong-version tree
  differs only in `manifest.json`'s `format_version` value; the missing-rollup tree
  differs only by the one absent file) — enforcing LOW-2's "one surgical mutation"
  invariant at generation time.
- `conformance/generator/regenerate.sh` — extended to derive both invalids from the
  baseline and run the "exactly one mutation" self-assertion.
- `conformance/invalid/wrong-format-version/` and
  `conformance/invalid/missing-root-rollup/` — the committed derived trees.

**Test plan.**
- Run `regenerate.sh`; both invalids are derived and the "differs in exactly one way"
  self-assertion passes for each.
- Inspect: `wrong-format-version/manifest.json` has `format_version: "0.2"` and is
  otherwise identical to the baseline manifest; `missing-root-rollup/` is missing
  exactly one root rollup and is otherwise byte-identical to the baseline.
- Confirm (manually) that hand-editing is unnecessary and disallowed: each invalid is
  reproduced solely by re-running the generator (LOW-2).
- **Rust gate:** `cargo build`, `cargo test`,
  `cargo clippy --all-targets -- -D warnings` all pass (Rust unchanged).

**Acceptance.**
- `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings` all pass.
- Two invalid trees exist under `conformance/invalid/`, each **derived
  programmatically** from the valid baseline via exactly one surgical mutation
  (LOW-2), each pinning exactly one spec check (M2; L1).
- The "differs in exactly one way" self-assertion passes for both, enforcing the
  one-mutation invariant at generation time.
- No hand-edited trees anywhere; both invalids are regenerable deterministically.
- Patch-bump + stage `Cargo.toml` + conventional commit + `git tag v<version>`.

**Spec refs.** §0/§14 M2 (hard cut — `wrong-format-version`), §4/§14 L1 (root rollups
exist — `missing-root-rollup`), §10/R2 (dev-only generator; two minimal invalids),
§11 (manifest shape for the mutated version field).

**Commit message.** `feat(conformance): derive two minimal invalid fixtures from baseline`

---

### MS2-S5 — Finalize conformance README + end-to-end deterministic regenerate

**Intent.** Document the complete fixture set for an agent landing in `conformance/`,
and wire `regenerate.sh` into a single deterministic end-to-end target that rebuilds
**all three** trees and runs **every** self-assertion (aborting on any failure).
Completes the MS2 deliverable set: the layout of each fixture, the regenerate command,
the **dev-only/not-a-writer** rule, the **LOW-2** hard rule, the **MED-5** MS3/MS4
hand-off, and the **check-id → invalid-fixture** table. Independently committable:
docs + the regenerate target; Rust untouched and green. Placed last so the README
reflects the final, complete set.

**Changes.**
- `conformance/README.md` — finalized:
  - **Layout** of each tree: `valid/minimal/` (annotated four-quadrant tree with the
    shared-grid-label COG+Zarr, the `time` axis, the field names incl. the
    `{source}_{variable}` + companion-mask patterns), `invalid/wrong-format-version/`,
    `invalid/missing-root-rollup/`.
  - **Regenerate command**: `conformance/generator/regenerate.sh` rebuilds everything
    deterministically and runs all self-assertions.
  - **Dev-only / not-a-writer** rule (restated, milestones.md MS2 risk).
  - **LOW-2 hard rule** (restated, load-bearing): invalid fixtures are derived from
    the single valid baseline via one surgical mutation; **never hand-edit a fixture
    tree** — add a mutation and regenerate.
  - **MED-5 hand-off** (restated as a named section): parquet `time` row-group
    statistics → **confirmed in MS3 (Rust)**; Zarr consolidated metadata → **confirmed
    in MS4 (Rust)**; a Rust/writer mismatch is fixed by **regenerating the fixture**,
    never a reader workaround.
  - **Check-id → invalid-fixture table**: `M2 → invalid/wrong-format-version/`,
    `L1 → invalid/missing-root-rollup/` (with which rollup is removed), and a note
    that the **exhaustive one-per-check** invalid family is **MS8**, not MS2.
  - A **seeding table** listing which on-disk preconditions the valid fixture seeds
    for each later-enforced check (L1, L2, L3, I1, I2, I3, H1, H2, T1, T2, G1, G2, G3,
    Geo1, M5, M6), explicitly labeled **seeding, not enforcement**.
- `conformance/generator/regenerate.sh` — finalized: one command builds
  `valid/minimal/` (scalar S2 + gridded S3), then derives both invalids (S4), then
  runs **all** self-assertions and **exits non-zero if any fail** (so a broken
  property aborts regeneration — milestones.md MS2 exit criterion).

**Test plan.**
- Run `conformance/generator/regenerate.sh` from a clean state: it produces a
  byte-deterministic `valid/minimal/` + both `invalid/*` trees, runs every
  self-assertion, and exits 0. Re-running yields an identical tree (determinism).
- Verify a self-assertion failure aborts (locally break one property, confirm
  non-zero exit), then revert.
- Confirm `conformance/README.md` contains: per-tree layout, regenerate command,
  dev-only/not-a-writer rule, LOW-2 hard rule, the MED-5 named hand-off, the check-id
  → invalid-fixture table, and the seeding table.
- **Rust gate:** `cargo build`, `cargo test`,
  `cargo clippy --all-targets -- -D warnings` all pass (Rust unchanged).

**Acceptance.**
- `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings` all pass.
- `regenerate.sh` rebuilds all three trees deterministically, runs **every**
  self-assertion, and **aborts on any failure** (milestones.md MS2 exit criterion).
- `conformance/README.md` documents each fixture's layout, the regenerate command,
  the **dev-only/not-a-writer** rule, the **LOW-2** hard rule, the **MED-5** named
  MS3/MS4 hand-off, the check-id → invalid-fixture table (M2; L1), and the
  seeding-not-enforcement table for the later-enforced checks.
- One valid + two invalid datasets are committed under `conformance/`; each invalid
  is documented as violating exactly one spec check.
- Patch-bump + stage `Cargo.toml` + conventional commit + `git tag v<version>`.

**Spec refs.** §2–§9 (the documented fixture properties), §10/R2 (no writer; dev-only
generator; regenerate script), §11 (six-field manifest), §14 (the seeded check ids +
the two pinned invalids), milestones.md MS2 (README content, regenerate determinism,
self-assertion abort).

**Commit message.** `docs(conformance): document fixtures and wire deterministic regenerate`

---

## Coverage check — every MS2 deliverable & exit criterion is assigned

| MS2 deliverable / exit criterion | Step |
|---|---|
| `conformance/generator/` Python project with **pinned** deps | S1 |
| `Makefile`/`regenerate.sh` deterministic rebuild target | S1 (stub) → S5 (end-to-end) |
| Dev-only / not-a-writer rule in `conformance/README.md` | S1 (stated) → S5 (finalized) |
| `conformance/valid/minimal/` — ≥2 basins, **all four quadrants** | S2 (scalar·static + scalar·dynamic) + S3 (gridded·static + gridded·dynamic) |
| COG + Zarr **share one grid label** and are **cell-for-cell aligned** (G2 precondition) | S3 |
| Generator self-assertion: shared label + alignment | S3 |
| **Ragged-across-basins** time (§6.1); **aligned-within-basin** time (§6.2) | S2 (ragged-across) + S3 (intra-basin scalar↔Zarr alignment) |
| Generator self-assertion: §6.1 ragged + §6.2 alignment | S2 + S3 |
| In-file `basin_id` == folder + unique (I2/I3 seed) + self-assertion | S2 |
| `time` full timestamp, non-null, sorted (T1) + **row-group statistics** (§8) + self-assertion | S2 |
| Zarr **consolidated metadata** + **v3 sharding** (§8) + self-assertion | S3 |
| Companion-mask + `{source}_{variable}` fields present (ordinary) + self-assertion | S3 |
| `outlines.geoparquet` `(basin_id, delineation, geometry)`, ≥2 labels, not partitioned (Geo1 seed) | S2 |
| `invalid/wrong-format-version/` (pins M2) — **derived, one mutation** | S4 |
| `invalid/missing-root-rollup/` (pins L1) — **derived, one mutation** | S4 |
| `conformance/README.md` — layout, regenerate, dev-only, check-id table | S5 (+ S1 first version) |
| Generator self-asserts **every** engineered property; failure aborts generation | S2 + S3 + S4 (per-property) → S5 (end-to-end abort) |
| Seeds L1, L2, L3, I1, I2, I3, H1, H2, T1, T2, G1, G2, G3, Geo1, M5, M6 (on-disk preconditions) | S2 (L1, I1–I3, T1, Geo1, M5, M6-time) + S3 (L2, H1, H2, G1, G2, G3, T2) → S5 (seeding table) |
| **No Rust build change**; crate stays green | S1–S5 (every step; Rust untouched, gate re-run) |
| Bump+tag commit discipline | S1–S5 (every step) |
| Inert/agnostic; six-field floor; delineation-neutral grids; no special-casing | S1–S5 (scope guard, per step) |
| **MED-5** writer/reader linkage: at-risk props named as MS3/MS4 Rust-side hand-off | S2 (`time` stats → MS3) + S3 (consolidated metadata → MS4) + S5 (named README section) |
| **LOW-2** derived-not-hand-authored hard rule recorded | S1 (stated) + S4 (enforced by "one mutation" self-assertion) + S5 (finalized in README) |

**Exit-criteria spec MUST-checks (MS2 SEEDS; enforced later in MS3–MS6):** the valid
fixture provides on-disk preconditions for L1, L2, L3, I1, I2, I3, H1, H2, T1, T2,
G1, G2, G3, Geo1, M5, M6; the two invalids pin M2 (`wrong-format-version`) and L1
(`missing-root-rollup`). MS2 ships **no Rust and enforces nothing** — this is a
*seeding* claim. Enforcement is MS6; the exhaustive one-per-check invalid family is
MS8.
