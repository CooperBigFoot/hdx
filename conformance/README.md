# HDX conformance fixtures

This directory holds the on-disk conformance fixtures for HDX v0.1 — **one valid
dataset plus two minimal invalid datasets** — and the dev-only Python harness
that generates them under [`generator/`](generator/).

There is **no HDX writer in v0.1** (spec §10; architecture §7 **R2**): `validate`
and `describe` are read-only. Yet MS3/MS4 readers and MS6 `validate` need real
on-disk parquet / Zarr / COG / geoparquet bytes to test against. This harness
fills that gap by *emitting bytes a reader will later read* — it is a test fixture
tool, **not** part of the shipped contract.

> **Tracking policy (IMPORTANT).** The generated fixture **data** under
> `valid/` and `invalid/` is **git-ignored, not committed** — it is reproduced
> deterministically from the committed generator. **Run
> `conformance/generator/regenerate.sh` before `cargo test`** (the Rust tests read
> these trees from disk; a fresh checkout has none until you regenerate). What
> *is* tracked: the generator source, this README, and the small golden
> assertion baselines under **`conformance/goldens/`** (each a
> `<fixture>.<verb>.json` file produced by the Rust verb — the stable,
> human-reviewable snapshot the tests assert against). The goldens live
> **OUTSIDE** the gitignored `valid/`/`invalid/` trees on purpose: `regenerate.sh`
> wipes (`rmtree`s) and rewrites those trees, so a golden inside them would be
> deleted on the next regenerate — `conformance/goldens/` is a directory the
> generator never touches. This keeps binary fixture blobs out of git history
> while preserving reproducibility.

The fixture set is **complete** for MS2: one valid four-quadrant dataset and the
two pinned invalids (`wrong-format-version`, `missing-root-rollup`). The
exhaustive one-invalid-per-check family is a later milestone (**MS8**), not this
one.

## Regenerate

```sh
conformance/generator/regenerate.sh
# or, to pin the interpreter explicitly (recommended on hosts that ship 3.13/3.14):
PYTHON=python3.12 conformance/generator/regenerate.sh
# verbose diagnostics:
HDX_FIXTURES_LOG_LEVEL=DEBUG conformance/generator/regenerate.sh
```

[`generator/regenerate.sh`](generator/regenerate.sh) is **the single end-to-end
target**. One run:

1. idempotently creates a pinned venv (CPython **3.12.x**; see
   [`generator/pyproject.toml`](generator/pyproject.toml)) and installs the
   exact-version closure from
   [`generator/requirements.lock`](generator/requirements.lock);
2. smoke-imports every pinned dependency (proving the pins resolve);
3. emits the valid baseline `valid/minimal/` — the **scalar** half (S2) then the
   **gridded** half (S3) — and **derives both invalids** (S4) from it;
4. runs **every** load-bearing self-assertion and **exits non-zero if any
   fails** — a broken property aborts the whole regenerate, so a non-conformant
   tree is never produced (milestones.md MS2 exit criterion).

**Determinism.** A run is **byte-deterministic**: `created_at` is a fixed
constant, every data series is a deterministic function of basin identity, and
the Zarr root `zarr.json`'s consolidated-metadata members are sorted to a stable
order (`zarr.consolidate_metadata` otherwise orders them non-deterministically;
see `grids._stabilize_consolidated_metadata`). Re-running yields a **bit-for-bit
identical** tree, so a regenerate produces no spurious diff.

---

## Layout

The `valid/`/`invalid/` trees are *generated* (git-ignored — see the tracking
policy above). Each is one mutation (or zero, for the baseline) off a known-good
four-quadrant dataset. The committed golden snapshots live OUTSIDE those trees,
under `conformance/goldens/`, so `regenerate.sh` never clobbers them.

```
conformance/
  README.md                              # this file
  generator/                             # dev-only Python harness (NOT shipped in hdx-core)
    pyproject.toml                        # pinned deps + interpreter (CPython 3.12.x)
    requirements.lock                     # exact-version lock installed by regenerate.sh
    regenerate.sh                         # the single deterministic end-to-end target
    hdx_fixtures/                         # generator package (manifest/scalar/outlines/grids/mutate/assertions)
  goldens/                                # TRACKED golden snapshots (outside the gitignored trees; regenerate-inert)
    valid-minimal.describe.json            # pinned `describe` output (R4); produced by hdx-core, not the generator
    valid-minimal.validate.json            # pinned `validate` report (R4); produced by hdx-core, not the generator
    invalid-<fixture>.validate.json         # pinned per-fixture `validate` report for each invalid (R4)
  valid/minimal/                          # the one valid four-quadrant dataset (git-ignored data)
  valid/irregular-time/                   # M6 still-conformant case (irregular time axis) — derived, STILL conformant:true (git-ignored data)
  invalid/wrong-format-version/           # pins M2 — one surgical mutation off the baseline (git-ignored data)
  invalid/missing-root-rollup/            # pins L1 — one surgical mutation off the baseline (git-ignored data)
```

> **Two valid-shaped fixtures.** `valid/minimal/` is the four-quadrant baseline.
> `valid/irregular-time/` (folder `valid/irregular-time-axis/`) is a SECOND
> valid-shaped fixture, derived from the baseline by the same one-mutation
> machinery but **STILL `conformant:true`** — it documents the
> no-enforceable-M6-negative finding (see [the M6 subsection
> below](#the-still-conformant-m6-case-no-enforceable-m6-negative-in-v01)). The
> suite cleanly separates these "derived from baseline but still conformant"
> fixtures (under `valid/`) from the fail-closed negatives (under `invalid/`).

### `valid/minimal/` — the one valid four-quadrant dataset

A basin-first hive (spec §4) over **three basins** (`basin=0001/0002/0003`),
spanning **all four field quadrants** (spec §2). Field names are opaque producer
strings — HDX attaches no role, units magic, or belongs-to link to any of them.

```
valid/minimal/
  manifest.json                           # EXACTLY the six floor fields (§11); format_version "0.1"
  scalar_static.parquet                   # ROOT rollup: 1 row/basin; cols (basin_id, drainage_area)   [scalar·static]
  outlines.geoparquet                     # ROOT rollup: rows (basin_id, delineation, geometry); plural labels (Geo1)
  basin=0001/
    scalar_dynamic.parquet                # rows=time; cols (basin_id, time, streamflow)               [scalar·dynamic]
    gridded_static/
      era5.tif                            # multiband COG; band description = "elevation"             [gridded·static]
    gridded_dynamic/
      era5.zarr/                          # Zarr v3; CF vars "era5_precipitation" (+ companion mask)   [gridded·dynamic]
  basin=0002/ ...                         # same field schema (H1) + same grid-label set (H2)
  basin=0003/ ...
```

Annotated, engineered properties:

- **Four quadrants.** `scalar_static` (`[]`), `scalar_dynamic` (`[T]`),
  `gridded_static` COG (`[Y,X]`), `gridded_dynamic` Zarr (`[T,Y,X]`).
- **Shared grid label ⇒ alignment (G2).** The COG and the Zarr in each basin both
  use the single grid label **`era5`** (`era5.tif` / `era5.zarr`) and are written
  from the **same affine / extent / resolution**, so they are **cell-for-cell
  aligned** — the G2 positive-path precondition.
- **The `time` axis.** Each `scalar_dynamic.parquet` carries a `time` column that
  is a **full timestamp**, **non-nullable**, **sorted ascending**, written **with
  row-group statistics** (T1, §8). Within each basin the Zarr `time` is the
  **identical** axis (T2, §6.2). Across basins the time extents are **ragged**
  (different periods of record, §6.1).
- **Field names — ordinary, no magic.** The gridded·dynamic field uses the
  `{source}_{variable}` pattern (**`era5_precipitation`**) and ships alongside a
  companion-mask field using the `{field}_was_filled` pattern
  (**`era5_precipitation_was_filled`**). Both are **ordinary** Zarr variables —
  the generator attaches no role, belongs-to link, or suffix/prefix magic. They
  exist only to prove later milestones give them **no** special handling (§2).
  The §6.2 NaN-fill convention is exercised: the first timestep of
  `era5_precipitation` is NaN and the mask marks exactly that timestep.
- **Delineation-neutral grids (§9).** Both gridded artifacts are dense
  rectangular over the basin bbox — never clipped or NaN'd to an outline.
  `outlines.geoparquet` carries **plural** delineations (`merit`, plus a `grit`
  for `basin=0001`) as neutral labels, in a single non-partitioned root file.

### `goldens/valid-minimal.describe.json` — the pinned `describe` output (R4)

A single committed golden file holds the exact `describe` JSON output for the valid
fixture: [`goldens/valid-minimal.describe.json`](goldens/valid-minimal.describe.json).
It lives under `conformance/goldens/`, OUTSIDE the gitignored fixture trees, so a
`regenerate.sh` (which rmtrees `valid/`/`invalid/`) never clobbers it.

It is **produced by `hdx-core`'s `describe` verb — NOT by the Python generator.** The
generator emits the on-disk dataset bytes; `describe` reads them and assembles the
self-description (manifest + discovered facts, no verdict). The golden is the
pretty-printed output of `describe_json(valid/minimal)` and is pinned by two Rust tests
in `crates/core/src/describe.rs`:

1. it **validates** against [`schemas/describe.schema.json`](../schemas/describe.schema.json)
   (via the test-only `jsonschema` dev-dep) — the R4 describe-half lock;
2. `describe` of the valid fixture, parsed to JSON, **equals** the golden — the snapshot.

The golden is where the companion-mask (`era5_precipitation_was_filled`) and the
`{source}_{variable}` (`era5_precipitation`) fields are pinned as **ordinary** catalog
entries: each carries exactly `{name, quadrant, dtype, units, grid_label}`, with no
`mask` / `companion` / `source` / `variable` / `belongs_to` key (spec §2 — no special
handling).

**Versioned implicitly by `format_version` only.** The describe shape carries no
schema-version field; its only version is the manifest `format_version` hard cut (spec
§0/§11). A shape change is therefore a `format_version` bump, and the golden is
refreshed in the same change.

**Golden-update workflow.** The golden is regenerated **from the Rust verb**, never
hand-edited: run `describe` over `valid/minimal`, pretty-print it
(`Description::to_json_pretty`), and overwrite
`conformance/goldens/valid-minimal.describe.json`. Do this **only** when the describe
shape legitimately changes (a `format_version` bump). A drift caught by the snapshot
test that is **not** an intended shape change is a bug, not a golden-refresh. (MS8
extends this golden-output discipline to the wider fixture family.)

> **MS8 adds no field.** The exhaustive-invalids milestone introduces no new domain
> field and mutates no manifest floor field, so this baseline
> `goldens/valid-minimal.describe.json` is **byte-unchanged** across MS8 — the green
> floor every MS8 invalid fixture builds on.

### `goldens/valid-minimal.validate.json` — the pinned `validate` report (R4)

A single committed golden file holds the exact `validate` report JSON for the valid
fixture: [`goldens/valid-minimal.validate.json`](goldens/valid-minimal.validate.json).
It lives under `conformance/goldens/`, OUTSIDE the gitignored fixture trees, so a
`regenerate.sh` (which rmtrees `valid/`/`invalid/`) never clobbers it. Each invalid
fixture has its own pinned report at `goldens/invalid-<fixture>.validate.json`.

It is **produced by `hdx-core`'s `validate` verb — NOT by the Python generator.** The
generator emits the on-disk dataset bytes; `validate` reads them, runs the §14 `MUST`
checklist over the discovery layer, and emits the report (the per-check outcomes +
`conformant`). The golden is the pretty-printed output of `validate_json(valid/minimal)`
and is pinned by Rust tests in `crates/core/src/validate.rs`:

1. it **validates** against [`schemas/validate.schema.json`](../schemas/validate.schema.json)
   (via the test-only `jsonschema` dev-dep) — the R4 validate-half lock;
2. `validate` of the valid fixture, parsed to JSON, **equals** the golden — the snapshot.

**It records which checks ran vs were skipped (spec §14 note).** The golden lists **all
20** §14 ids; each carries its `status` (`ran` / `skipped`), its `result` (`pass` / `fail`,
or `null` for a skip), its R3 `depth` (`metadata_deep` / `byte_deep`), and an opaque
`detail`. On the valid fixture every check `ran:pass` **except** the v0.1 honest R3 skips —
**M6** (per-basin axis regularity needs the full 1-D `time` array), **L3** (the
absence-is-NaN-not-a-missing-file leg needs a byte-deep payload read), and **T2** (the
cross-artifact full time-axis identity needs both 1-D axes) — each with a non-empty skip
reason. `conformant` is `true` (a skip never flips the verdict; fail-closed applies only to
a violated `MUST` that ran). This makes the §14-note requirement ("the validator MUST
clearly report which checks ran") a **machine-readable, pinned artifact**.

**Versioned implicitly by `format_version` only.** Like the describe shape, the validate
report carries no schema-version field; its only version is the manifest `format_version`
hard cut (spec §0/§11). A shape change is therefore a `format_version` bump, refreshed in
the same change.

**Golden-update workflow.** The golden is regenerated **from the Rust verb**, never
hand-edited: run `validate` over the fixture, pretty-print it
(`ValidationReport::to_json_pretty`), and overwrite the matching golden under
`conformance/goldens/` — `valid-minimal.validate.json` for the valid baseline, or
`invalid-<fixture>.validate.json` for an invalid (the fixture path flattened with
`/` → `-`). Do this **only** when the report shape legitimately changes (a
`format_version` bump). A drift caught by the snapshot test that is **not** an
intended shape change is a bug, not a golden-refresh. (MS8 extends this
golden-output discipline to the wider invalid fixture family — the exhaustive
one-violation-per-check golden report matrix.)

> **MS8 adds no field.** The exhaustive-invalids milestone introduces no new domain
> field and mutates no manifest floor field, so this baseline
> `goldens/valid-minimal.validate.json` is **byte-unchanged** across MS8 — the green
> floor every MS8 invalid fixture builds on.

### The still-conformant M6 case: no enforceable M6 negative in v0.1

`valid/irregular-time-axis/` is the documented M6 still-conformant fixture, derived
from the baseline by **exactly one surgical mutation**: ONE basin's
`scalar_dynamic.parquet` `time` column is rewritten to an **irregular** but
**strictly-ascending, non-null** axis (days `[0,1,3,7]` off the basin's start
instead of the baseline `[0,1,2,3]` — gaps of 1, 2, 4 days), **and** that basin's
`gridded_dynamic` Zarr `time` coordinate is re-emitted to the **identical**
irregular axis (so the intra-basin scalar/gridded axes stay equal and T2 does not
spuriously co-fail). The point COUNT is unchanged, so the parquet rows and the
Zarr `time` length still match. The generator self-assertion
(`assertions._assert_irregular_time_axis`) proves the matched pair — exactly that
basin's `scalar_dynamic.parquet` and its Zarr `time` differ, the axis is strictly
ascending with **non-uniform** gaps, the Zarr axis equals the scalar axis, and no
file is added or removed.

`validate` of this fixture reports **M6 `status:skipped`** with a non-empty reason
naming the regularity leg, `result:null`, and top-level **`conformant:true`** — the
golden is [`goldens/valid-irregular-time-axis.validate.json`](goldens/valid-irregular-time-axis.validate.json),
byte-identical to the baseline `valid-minimal` validate golden (the irregular
spacing is byte-deep invisible to `validate`). The pinned regression test is
`irregular_time_axis_skips_m6_and_stays_conformant` in `crates/core/src/validate.rs`.

**Why there is NO enforceable M6 negative in v0.1.** `check_m6` is exactly two
rules (FOLD MED-1): rule (a) — `cadence` is a non-empty string (this also is M4;
M6 references it) — and rule (b) — each basin's realized `time` axis is
**internally regular** (a constant interior step, the §6.2 consequence of
NaN-filled gaps). Rule (b) is honestly **R3 `ByteDeep`-skipped**: the v0.1
discovery model surfaces only a **two-point `[start, end]` `TimeExtent`** plus a
`sorted_ascending` flag, **from which a constant interior step is not derivable**
(you would need the full 1-D `time` array). Because a `Skipped` leg is never a
fail and rule (a) passes, an irregular per-basin axis stays `conformant:true`.

Two hard guardrails this fixture documents and the test pins:

- **M6 never interprets the cadence *word*.** It does NOT read `"daily"` as a
  1-day step (that would be the semantic interpretation HDX must avoid, spec
  §1/§6.4). The skip reason names *axis regularity*, never the cadence word.
- **M6 asserts no cross-basin step equality.** §6.1 explicitly permits **ragged
  per-basin time extents**, so a merely-different cross-basin step is not a
  failure. No cross-basin cadence rule is resurrected; the reason states this
  explicitly ("no cross-basin step equality asserted").

So the only M6 *fail* form is an empty cadence (rule (a)) — and that is already
rejected at the M4 entry gate before `check_m6` runs (the `empty-cadence` invalid
pins M4, not M6). There is therefore **no fixture that makes M6 `ran:fail`** in
v0.1; the irregular-time-axis fixture is the documented still-conformant case in
its place.

### `invalid/wrong-format-version/` — pins **M2**

Byte-identical to the baseline **except** `manifest.json`'s `format_version` is
`"0.2"` instead of `"0.1"`. M2 is the §0 **hard version cut**: any value other
than `"0.1"` is rejected outright. Exactly **one** file (`manifest.json`) differs,
and it differs **only** in that one value — the other five floor fields, and every
other file in the tree, are byte-identical to the baseline.

### `invalid/missing-root-rollup/` — pins **L1**

Byte-identical to the baseline **except** the root **`outlines.geoparquet`** is
**deleted**. L1 requires *both* root rollups (`scalar_static.parquet` **and**
`outlines.geoparquet`) to exist; this removes exactly one of them. Exactly **one**
file is absent; nothing is added, and no remaining file's bytes differ.

---

## Check-id → invalid-fixture table

Each invalid is **derived programmatically** from the valid baseline via **exactly
one surgical mutation** (LOW-2) and pins **exactly one** spec §14 check.

| Spec check (§14) | Invalid fixture | The one mutation |
|---|---|---|
| **M2** — `format_version == "0.1"`; any other value is rejected outright (hard cut). | `invalid/wrong-format-version/` | `manifest.json` `format_version`: `"0.1"` → `"0.2"` (all other fields unchanged). |
| **L1** — `scalar_static.parquet` and `outlines.geoparquet` exist at the root. | `invalid/missing-root-rollup/` | Delete the root **`outlines.geoparquet`** (the other rollup, `scalar_static.parquet`, is kept). |
| **M5** — the manifest `crs` matches every georeferenced file's recorded CRS (§7/§11). | `invalid/crs-mismatch/` | `manifest.json` `crs`: `"EPSG:4326"` → `"EPSG:3857"` (the files keep `EPSG:4326`; all other floor fields unchanged). |
| **G2** — a grid label shared across the COG and Zarr subtrees implies cell-for-cell alignment (§8). | `invalid/misaligned-shared-label/` | Re-emit one basin's `gridded_static/era5.tif` under the **same** `era5` label at a half-cell-shifted geometry (`west` `10.0` → `10.5`); its Zarr stays at the baseline geometry, so the shared label no longer coincides. |
| **H2** — the grid-label set is identical across basins (§8). | `invalid/divergent-grid-label-set/` | Re-emit one basin's COG **and** Zarr under a divergent `era5b` label (`era5.*` → `era5b.*`); that basin's label set becomes `{era5b}` while every other basin's is `{era5}`. |
| **M6 (skip)** — *no enforceable negative*; M6 rule (b) regularity is R3-skipped. STILL **`conformant:true`** (a valid-shaped fixture, not a fail-closed invalid). | `valid/irregular-time-axis/` | Rewrite one basin's `scalar_dynamic` `time` to an irregular but strictly-ascending, non-null axis (days `[0,1,3,7]`) **and** its matching Zarr `time` to the identical axis. See [the M6 subsection](#the-still-conformant-m6-case-no-enforceable-m6-negative-in-v01). |

> **The exhaustive one-invalid-per-check family is MS8.** MS2 shipped the first
> two pinned invalids (M2, L1); MS8 adds the rest (the entry-gate M3/M4, the
> Bucket-B I1/I2/H1/T1/L2 parquet/layout negatives, and the M5/G2/H2 georef /
> grid-label negatives shown above). The full classification matrix is finalized
> in MS8-S4. Every invalid is added the same way: add a mutation to the generator
> and regenerate (never hand-edit a tree).

---

## Seeding, not enforcement

MS2 ships **no Rust and enforces nothing.** The valid fixture engineers the
on-disk **preconditions** for the later-enforced §14 checks; **enforcement is
MS6.** The table below maps each seeded check to the property the valid fixture
provides and where it is engineered. This is a **seeding** claim — read "seeds the
precondition for", never "enforces".

| Check (§14) | Seeded on-disk precondition (the valid fixture provides…) | Where engineered |
|---|---|---|
| **L1** | both root rollups (`scalar_static.parquet`, `outlines.geoparquet`) present | scalar + outlines (S2) |
| **L2** | every basin dir is `basin=<id>` with `scalar_dynamic.parquet` + `gridded_static/`/`gridded_dynamic/` | scalar (S2) + grids (S3) |
| **L3** | no stray/ragged files; a field's gap is NaN-filled, never a missing file (the Zarr NaN-fill) | grids (S3) |
| **I1** | `basin_id` is a real in-file column in `scalar_static`, every `scalar_dynamic`, and `outlines` | scalar + outlines (S2) |
| **I2** | in-file `basin_id` agrees with the `basin=<id>` folder for every basin | scalar (S2) |
| **I3** | `basin_id` is unique across the dataset (one rollup row per basin) | scalar (S2) |
| **H1** | every basin carries the identical field schema (same names, dtypes, quadrants) | scalar (S2) + grids (S3) |
| **H2** | the grid-label set (`{era5}`) is identical across basins | grids (S3) |
| **T1** | the scalar `time` column is named `time`, full timestamp, non-nullable, sorted ascending | scalar (S2) |
| **T2** | within each basin the `scalar_dynamic` and Zarr `gridded_dynamic` share the identical time axis; gaps NaN-filled | scalar (S2) + grids (S3) |
| **G1** | one artifact = one grid; fields self-name (COG band description / CF variable = field name); no positional channel axis | grids (S3) |
| **G2** | a shared grid label across the static/dynamic subtrees that **does** exhibit cell-for-cell alignment | grids (S3) |
| **G3** | Zarr CF georef (explicit `lat`/`lon` + `grid_mapping`); COG standard georeferencing tags | grids (S3) |
| **Geo1** | `outlines.geoparquet` rows `(basin_id, delineation, geometry)`; label column `delineation`; not partitioned | outlines (S2) |
| **M5** | the manifest `crs` (EPSG:4326) matches the CRS carried in every georeferenced file | manifest (S2) + grids (S3) |
| **M6** | the manifest `cadence` ("daily") is consistent with the realized daily `time` axes | manifest + scalar (S2) |

> The valid fixture also carries the two **MED-5** at-risk properties — parquet
> `time` **row-group statistics** (§8) and Zarr **consolidated metadata** (§8) —
> which §8 mandates and which MS3/MS4 confirm from the Rust side (see Rule 3).

---

## The three load-bearing rules

These rules are contract, not afterthought. They are also restated in the
generator source
([`generator/hdx_fixtures/__init__.py`](generator/hdx_fixtures/__init__.py)).

### Rule 1 — The generator is DEV-ONLY and is NOT an HDX writer

The generator lives **only** under `conformance/generator/`. It is:

- **never shipped in `hdx-core`**, never imported by, linked from, or depended on
  by any Rust crate or production code;
- **not an HDX writer** — HDX defines no writer in v0.1. The generator does not
  implement or execute any contract logic (that lives exclusively in `hdx-core`,
  per architecture §2). It engineers the on-disk *preconditions* for the spec
  checks so MS3–MS6 can read and enforce them; it enforces nothing itself.

Its own checks are **writer-side self-assertions** (Python), distinct from the
Rust-side enforcement in `validate`. Diagnostics in the generator go through the
standard `logging` machinery (to stderr), never raw `print`; the single
user-facing status line is *output* — mirroring the architecture §2 split between
diagnostics and output.

This is the milestones.md MS2 "generator masquerading as a writer" risk, closed
explicitly.

### Rule 2 — LOW-2: derived, not hand-authored (HARD RULE)

Every invalid fixture (and the larger MS8 invalid family later) **MUST** be
generated **programmatically from the single valid baseline via exactly one
surgical mutation each**. The generator builds the valid baseline once, then
derives each invalid by applying one targeted mutation (e.g. overwrite
`manifest.json`'s `format_version`; delete one root rollup).

> **A contributor MUST NOT hand-edit a fixture tree.** To add or change an
> invalid fixture, add a mutation to the generator and **regenerate**.

This keeps every fixture exactly one mutation off a known-good baseline, so
"differs in exactly one way" is true by construction and the whole suite is
maintainable as one generator rather than N hand-built trees. A generation-time
self-assertion (`assertions.assert_differs_in_exactly_one_way`) confirms each
invalid differs from the baseline in exactly the one intended way, aborting the
regenerate otherwise.

### Rule 3 — MED-5: Rust-side confirmation hand-off (MS3 / MS4)

The generator's self-assertions are **Python-side**: they assert what the
*writer* intended, which cannot prove what a *Rust reader* recovers from the same
bytes. Two engineered properties are most at risk of a writer/reader mismatch:

1. **Parquet `time` row-group statistics → confirmed in MS3 (Rust).** pyarrow may
   or may not emit usable min/max statistics for the timestamp logical type under
   the chosen settings. The generator self-asserts the *written file* carries them
   (`assertions.assert_time_column_and_statistics`); **MS3 MUST confirm from the
   Rust side** (`arrow`/`parquet`) that the time extent is sourced from those
   statistics (not a bounded-scan fallback) on the valid fixture.
2. **Zarr v3 consolidated metadata → confirmed in MS4 (Rust).** `zarr-python`'s v3
   consolidated-metadata layout must be readable by Rust `zarrs`. The generator
   self-asserts consolidated metadata is present
   (`assertions.assert_zarr_consolidated_and_sharded`); **MS4 MUST confirm from
   the Rust side** that it reads the store's metadata via the §8 consolidated path
   (or explicitly classify it an R3 byte-deep skip, with a stated reason).

> **The hand-off rule:** if MS3/MS4 find the Rust reader cannot recover a property
> the generator asserted, the fix is to **REGENERATE the fixture** (adjust the
> generator and re-emit) — **never** to add a reader workaround. A mismatch is a
> generator bug, not a reader bug.

---

## Inert / agnostic discipline

`manifest.json` is **exactly** the six floor fields (spec §11): `format_version`,
`name`, `created_at`, `producer_version`, `crs`, `cadence`. No content hash, no
data-version, no field catalog, no basin list, no transform/role/semantic/
provenance key. Field names are opaque producer strings; the `{source}_{variable}`
and companion-mask `{field}_was_filled` patterns appear **only to prove later
milestones give them no special handling**. `delineation` labels are neutral, not
trusted "hydrofabric" sources. `format_version` is a **hard cut** (`"0.1"` in the
baseline).
