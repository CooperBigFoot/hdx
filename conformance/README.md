# HDX conformance fixtures

This directory holds the on-disk conformance fixtures for HDX v0.1 — **one valid
dataset plus two minimal invalid datasets** — and the dev-only Python harness
that generates them under [`generator/`](generator/).

There is **no HDX writer in v0.1** (spec §10; architecture §7 **R2**): `validate`
and `describe` are read-only. Yet the `hdx-core` readers and `validate` need real
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

The fixture set covers the full **§14 matrix**: one valid four-quadrant
baseline and the **13** fail-closed invalids (including the HDX-0.2 M6 rule-(b)
negative `invalid/irregular-time-axis`) — one surgical mutation each, each pinning
exactly one §14 check. The full classification of all 20 §14 ids lives in the
[§14 check-id classification matrix](#14-check-id-classification-matrix).

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
3. emits the valid baseline `valid/minimal/` — the **scalar** half then the
   **gridded** half — and **derives every fixture the `mutate.Invalid` enum
   declares** from it (the 13 fail-closed invalids under `invalid/<name>/`,
   including the M6 rule-(b) negative `invalid/irregular-time-axis/` — one surgical
   mutation each);
4. runs **every** load-bearing self-assertion — including, per derived fixture,
   the `assert_differs_in_exactly_one_way` one-mutation check — and **exits
   non-zero if any fails**, so a broken property aborts the whole regenerate and a
   non-conformant tree is never produced.

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
    valid-multi_grid_multi_static.describe.json  # LOAD-BEARING merge-gen M1 proof: describe carries BOTH grid families' fields
    valid-multi_grid_multi_static.validate.json  # CORROBORATING-ONLY (field-catalog-insensitive; NOT the M1 proof)
    invalid-<fixture>.validate.json         # pinned per-fixture `validate` report for each invalid (R4)
  valid/minimal/                          # the one valid four-quadrant dataset (git-ignored data)
  valid/multi_grid_multi_static/          # merge-gen M1: two grid families per quadrant (dem+landcover static, era5+merit dynamic) (git-ignored data)
  invalid/irregular-time-axis/            # pins M6 (rule (b) per-basin axis regularity) — one surgical mutation off the baseline (git-ignored data)
  invalid/wrong-format-version/           # pins M2 (entry-gate Err) — one surgical mutation off the baseline (git-ignored data)
  invalid/extra-manifest-field/           # pins M3 (entry-gate Err) — one surgical mutation off the baseline (git-ignored data)
  invalid/empty-cadence/                  # pins M4 (entry-gate Err) — one surgical mutation off the baseline (git-ignored data)
  invalid/missing-root-rollup/            # pins L1 — one surgical mutation off the baseline (git-ignored data)
  invalid/missing-gridded-dynamic-subtree/ # pins L2 — one surgical mutation off the baseline (git-ignored data)
  invalid/missing-basin-id-column/        # pins I1 — one surgical mutation off the baseline (git-ignored data)
  invalid/basin-id-folder-mismatch/       # pins I2 — one surgical mutation off the baseline (git-ignored data)
  invalid/ragged-field-schema/            # pins H1 — one surgical mutation off the baseline (git-ignored data)
  invalid/non-monotonic-time/             # pins T1 — one surgical mutation off the baseline (git-ignored data)
  invalid/crs-mismatch/                   # pins M5 — one surgical mutation off the baseline (git-ignored data)
  invalid/misaligned-shared-label/        # pins G2 — one surgical mutation off the baseline (git-ignored data)
  invalid/divergent-grid-label-set/       # pins H2 — one surgical mutation off the baseline (git-ignored data)
```

> **The fixture set covers the full §14 matrix.** The 13 derived fixtures
> above (all fail-closed invalids, including the HDX-0.2 M6 rule-(b) negative
> `invalid/irregular-time-axis`) are exactly one per `mutate.Invalid` variant, and
> exactly the rows in the
> [fixture → one-pinned-check map](#fixture--one-pinned-check-map-every-committedgenerated-invalid)
> and the [§14 classification matrix](#14-check-id-classification-matrix). A clean
> `regenerate.sh` emits all of them; no fixture name elsewhere in this README is a
> dangling reference.

> **One valid-shaped fixture.** `valid/minimal/` is the four-quadrant baseline.
> Every other derived fixture is a fail-closed negative under `invalid/`. Under
> HDX 0.2 there is no still-conformant derived case: `invalid/irregular-time-axis`
> (the former pre-0.2 still-conformant M6 case) is now a fail-closed M6 rule-(b)
> negative (see [the M6 subsection
> below](#the-m6-rule-b-negative-irregular-time-axis-hdx-02)).

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
  exist only to prove the verbs give them **no** special handling (§2).
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
test that is **not** an intended shape change is a bug, not a golden-refresh. The
same golden-output discipline extends to the wider fixture family.

> **The invalid family adds no field.** No invalid fixture introduces a new
> domain field or mutates a manifest floor field, so this baseline
> `goldens/valid-minimal.describe.json` is the **byte-unchanged** green floor every
> invalid fixture is derived from (each invalid is one surgical mutation off this
> baseline). The invalid family pins `validate` reports only; it touches no
> `describe` golden.

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
`detail`. On the valid fixture **all 20** checks `ran:pass` (20/20, no skips) under HDX
0.2 — the byte-deep legs now run over the surfaced full per-basin axis + realized columns:
**T2** (cross-artifact full time-axis identity, both 1-D axes as i64 micros), **M6** rule
(b) (per-basin axis regularity over the full 1-D `time` array), and **L3** (absence-is-NaN,
not a missing file). `conformant` is `true`. This makes the §14-note requirement ("the
validator MUST clearly report which checks ran") a **machine-readable, pinned artifact**.

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
intended shape change is a bug, not a golden-refresh. The same golden-output
discipline extends to the wider invalid fixture family — the
one-violation-per-check golden report matrix.

> **The invalid family adds no field.** No invalid fixture introduces a new
> domain field or mutates a manifest floor field, so this baseline
> `goldens/valid-minimal.validate.json` is the **byte-unchanged** green floor every
> invalid fixture is derived from. Each fail-closed invalid gets its own
> `goldens/invalid-<fixture>.validate.json` pinning its single `ran:fail` report.

### `valid/multi_grid_multi_static/` — the merge-gen M1 field-catalog-completeness proof

A second valid-shaped four-quadrant dataset (three basins, all four quadrants, a
0.1 manifest, plural outlines — the `valid/minimal/` shape) whose ONLY difference
is the gridded half: it carries **TWO DISTINCT grid labels per gridded quadrant**.

* `gridded_static/` — two single-band COGs under labels **`dem`** (band field
  `dem_elevation`) and **`landcover`** (band field `landcover_class`).
* `gridded_dynamic/` — two Zarr v3 stores under labels **`era5`** (data var
  `era5_precipitation` + its companion mask) and **`merit`** (data var
  `merit_flow_accumulation` + its companion mask).

The static label set `{dem, landcover}` and the dynamic label set `{era5, merit}`
are disjoint, so **no label is shared across subtrees**: `check_g2` (which only
compares a shared label's COG+Zarr extents) finds no pair to compare and passes
trivially. Every basin carries the **same four labels**, so `check_h2` (cross-basin
label-set equality) stays pass. All four labels are emitted over the shared
baseline geometry/time axis, so they are georeferenced (G3) and time-aligned (T2).

This fixture exists to prove the merge-gen **M1 field-catalog completeness** fix
end-to-end through `describe`: the gridded field catalog must walk **ALL** static +
**ALL** dynamic artifacts and union their fields across both families (a
first-artifact-only catalog would surface only ONE static + ONE dynamic family's
field). The generator self-asserts (`run_multi_grid_multi_static_assertions`) that
every basin carries both static and both dynamic labels with their distinct fields,
and that the per-basin label set is homogeneous across basins.

* **`goldens/valid-multi_grid_multi_static.describe.json` — LOAD-BEARING (the M1
  proof).** Produced by `hdx-core`'s `describe` verb (regenerated like the
  `valid-minimal` describe golden — `Description::to_json_pretty`, never
  hand-edited). Its `fields[]` enumerates **BOTH families' fields** (both static
  labels' band fields `dem_elevation`+`landcover_class` AND both dynamic labels'
  data-var fields `era5_precipitation`+`merit_flow_accumulation`, plus companions).
  The Rust test `multi_grid_multi_static_describe_golden_carries_both_families`
  (`crates/core/src/describe.rs`) asserts `describe` equals this golden — the
  describe-completeness signal. It is **RED** on the pre-M1 first-artifact-only
  catalog (the 2nd family's fields are absent) and **GREEN** on the walk-all union.
* **`goldens/valid-multi_grid_multi_static.validate.json` — CORROBORATING-ONLY (NOT
  the M1 proof).** Produced by `hdx-core`'s `validate` verb. It is committed for
  completeness alongside the describe golden, but the validate report is
  **field-catalog-INSENSITIVE**: the catalog is consumed only by the
  order-insensitive `check_g1` (which only tests that a PRESENT gridded field
  self-names with `Some(GridLabel)` — a MISSING field cannot trip it). So the
  validate report is **byte-identical pre/post the M1 catalog fix** and is **NOT**
  the completeness signal. The describe golden is the proof; the validate golden
  only corroborates that the fixture is a conformant 0.2 dataset (conformant, no
  `ran:fail`, every skip carries a reason).

### The M6 rule-(b) negative: `invalid/irregular-time-axis` (HDX 0.2)

`invalid/irregular-time-axis/` is the M6 rule-(b) negative, derived from the
baseline by **exactly one surgical mutation**: ONE basin's
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

`validate` of this fixture reports **M6 `status:ran result:fail`** with a reason
naming the offending basin and the regularity leg, and top-level
**`conformant:false`** — the golden is
[`goldens/invalid-irregular-time-axis.validate.json`](goldens/invalid-irregular-time-axis.validate.json).
The pinned regression test is
`irregular_time_axis_fails_m6_rule_b_and_is_non_conformant` in
`crates/core/src/validate.rs`.

**How M6 enforces rule (b) under HDX 0.2.** `check_m6` is exactly two rules: rule
(a) — `cadence` is a non-empty string (this also is M4; M6 references it) — and
rule (b) — each basin's realized `time` axis is **strictly increasing** with a
**uniform interior step** (the §6.2 consequence of NaN-filled gaps). HDX 0.2
surfaces the full per-basin 1-D `time` axis on the discovery model (the gridded
`/time` int64-day decode normalized to i64 micros, or the scalar `time` column
projection), so rule (b) **runs over the full axis** — a constant interior step is
now derivable. An axis that is not strictly increasing or whose interior step is
non-constant ⇒ `ran:fail` naming that basin.

> **Pre-0.2 history.** In v0.1 the discovery model surfaced only a two-point
> `[start, end]` `TimeExtent` + a `sorted_ascending` flag, from which a constant
> interior step was not derivable, so rule (b) was honestly R3 `ByteDeep`-skipped
> and this fixture was a still-conformant `valid/irregular-time-axis` case. The 0.2
> unskip reclassified it to this fail-closed negative.

Two hard guardrails this fixture documents and the test pins:

- **M6 never interprets the cadence *word*.** It does NOT read `"daily"` as a
  1-day step (that would be the semantic interpretation HDX must avoid, spec
  §1/§6.4). The fail reason names *axis regularity*, never the cadence word.
- **M6 asserts no cross-basin step equality.** §6.1 explicitly permits **ragged
  per-basin time extents**, so a merely-different cross-basin step is not a
  failure — rule (b) is decided **per basin in isolation**. No cross-basin cadence
  rule is resurrected.

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

## Fixture → one-pinned-check map (every committed/generated invalid)

Each invalid is **derived programmatically** from the valid baseline via **exactly
one surgical mutation** (the generator's
`assert_differs_in_exactly_one_way` self-assertion proves the one-mutation
invariant at generation time) and pins **exactly one** spec §14 check. The table
lists **every** fixture a clean `regenerate.sh` emits beyond the valid baseline:
the **13 enforceable negatives** (a clean `conformant:false` report — or, for
M2/M3/M4, an entry-gate `Err` — with exactly one §14 check failing), including the
HDX-0.2 M6 rule-(b) negative `invalid/irregular-time-axis`. Each row
is the single fixture the generator's `Invalid` enum declares (one per
`mutate.Invalid` variant); no row names a fixture that is not emitted, and every
emitted fixture appears here.

| Spec check (§14) | Fixture | Negative kind | The one mutation |
|---|---|---|---|
| **M2** — `format_version == "0.1"`; any other value is rejected outright (hard cut). | `invalid/wrong-format-version/` | entry-gate `Err` | `manifest.json` `format_version`: `"0.1"` → `"0.2"` (all other fields unchanged). |
| **M3** — exactly the six floor fields are present; no derivable field (§11). | `invalid/extra-manifest-field/` | entry-gate `Err` | Append a 7th derivable key `content_hash` after the six floor fields (their values byte-identical). |
| **M4** — `created_at` is RFC 3339; `crs`, `cadence` are non-empty strings. | `invalid/empty-cadence/` | entry-gate `Err` | `manifest.json` `cadence`: `"daily"` → `""` (the other five floor fields byte-identical). |
| **L1** — `scalar_static.parquet` and `outlines.geoparquet` exist at the root. | `invalid/missing-root-rollup/` | `conformant:false` | Delete the root **`outlines.geoparquet`** (the other rollup, `scalar_static.parquet`, is kept). |
| **L2** — every basin carries its required per-basin artifacts (§4). | `invalid/missing-gridded-dynamic-subtree/` | `conformant:false` | Delete one basin's `gridded_dynamic/` subtree; the dataset still declares gridded·dynamic fields, so that basin's empty `dynamic_artifacts()` fails L2. |
| **I1** — `basin_id` is a real in-file column in every required artifact (§3). | `invalid/missing-basin-id-column/` | `conformant:false` | Drop the `basin_id` column from one basin's `scalar_dynamic.parquet` (the reader records `has_basin_id=false`, it does **not** error). |
| **I2** — in-file `basin_id` agrees with the `basin=<id>` folder (§3). | `invalid/basin-id-folder-mismatch/` | `conformant:false` | Rewrite one basin's in-file `basin_id` to a unique foreign value (`9999`) that disagrees with its `basin=<id>` folder. |
| **H1** — every basin has the identical field schema (§5). | `invalid/ragged-field-schema/` | `conformant:false` | Rename one basin's `scalar_dynamic` data field `streamflow` → `flow` (dtype/quadrant/nullability kept; only the name diverges). |
| **T1** — the scalar `time` column is named `time`, a timestamp, non-null, sorted ascending (§6.3). | `invalid/non-monotonic-time/` | `conformant:false` | Write one basin's `scalar_dynamic` `time` descending across row groups (`sorted_ascending` false). |
| **M5** — the manifest `crs` matches every georeferenced file's recorded CRS (§7/§11). | `invalid/crs-mismatch/` | `conformant:false` | `manifest.json` `crs`: `"EPSG:4326"` → `"EPSG:3857"` (the files keep `EPSG:4326`; all other floor fields unchanged). |
| **G2** — a grid label shared across the COG and Zarr subtrees implies cell-for-cell alignment (§8). | `invalid/misaligned-shared-label/` | `conformant:false` | Re-emit one basin's `gridded_static/era5.tif` under the **same** `era5` label at a half-cell-shifted geometry (`west` `10.0` → `10.5`); its Zarr stays at the baseline geometry, so the shared label no longer coincides. |
| **H2** — the grid-label set is identical across basins (§8). | `invalid/divergent-grid-label-set/` | `conformant:false` | Re-emit one basin's COG **and** Zarr under a divergent `era5b` label (`era5.*` → `era5b.*`); that basin's label set becomes `{era5b}` while every other basin's is `{era5}`. |
| **M6** — each basin's realized `time` axis is strictly increasing with a uniform interior step (rule (b), §6.2/§6.4). | `invalid/irregular-time-axis/` | `conformant:false` | Rewrite one basin's `scalar_dynamic` `time` to an irregular but strictly-ascending, non-null axis (days `[0,1,3,7]` — gaps 1,2,4) **and** its matching Zarr `time` to the identical axis; rule (b) runs over the full axis and fails the non-constant interior step. See [the M6 subsection](#the-m6-rule-b-negative-invalid-irregular-time-axis-hdx-02). |

> **The one-invalid-per-check family is exhaustive.** It spans the entry-gate
> M2/M3/M4, the I1/I2/H1/T1/L2 parquet/layout negatives, the M5/G2/H2
> georef/grid-label negatives, and the M6 rule-(b) regularity negative. **The 13
> rows above are exactly the fixtures a clean `regenerate.sh` emits** (one per
> `mutate.Invalid` variant). The full §14 classification matrix — every check id
> placed in exactly one of three buckets — is the [next section](#14-check-id-classification-matrix).
> Every invalid is added the same way: add a mutation to the generator and
> regenerate (never hand-edit a tree).

---

## §14 check-id classification matrix

The §14 `MUST` checklist has **20** ids (M1–M6, L1–L3, I1–I3, H1–H2, T1–T2,
G1–G3, Geo1). Every id falls into **exactly one** of three buckets, determined by
what [`crates/core/src/validate.rs`](../crates/core/src/validate.rs) `build_report`
actually does on the fixture set. **This matrix was confirmed by reading
`crates/core/src/validate.rs`** (the `build_report` function and each `check_*`
rule); the two buckets below are, respectively, the `ran:fail`-on-its-negative
set and the `ran:pass`-with-no-isolable-on-disk-negative set. Under HDX 0.2 there
are **no** R3 skips left — all 20 checks RUN (T2, M6 rule (b), and L3 read the
surfaced full per-basin axis + realized columns).

### (a) Enforced, with an on-disk negative — 13 ids

These checks **run** on every fixture and `ran:fail` on a dedicated on-disk
negative that is one surgical mutation off the baseline (the
[fixture map above](#fixture--one-pinned-check-map-every-committedgenerated-invalid)).
M2/M3/M4 fail at the §0 **entry gate** (an `Err` from `Manifest::from_json` before
`discover` runs); the rest are a clean `conformant:false` report with that single
check `ran:fail`.

| Id | What it enforces | On-disk negative (fixture) | `validate.rs` site |
|---|---|---|---|
| **M2** | `format_version == "0.1"` (hard cut) | `invalid/wrong-format-version/` | `Manifest::from_json` (entry gate, early `?` in `validate`) |
| **M3** | exactly the six floor fields | `invalid/extra-manifest-field/` | `Manifest::from_json` (entry gate) |
| **M4** | non-empty `crs`/`cadence`, RFC-3339 `created_at` | `invalid/empty-cadence/` | `Manifest::from_json` (entry gate) |
| **M5** | manifest `crs` == every file's CRS | `invalid/crs-mismatch/` | `check_m5` |
| **L1** | both root rollups present | `invalid/missing-root-rollup/` | `check_l1` |
| **L2** | every basin's required artifacts present | `invalid/missing-gridded-dynamic-subtree/` | `check_l2` |
| **I1** | `basin_id` column present in every required artifact | `invalid/missing-basin-id-column/` | `check_i1` |
| **I2** | in-file `basin_id` == folder | `invalid/basin-id-folder-mismatch/` | `check_i2` |
| **H1** | identical field schema across basins | `invalid/ragged-field-schema/` | `check_h1` |
| **H2** | identical grid-label set across basins | `invalid/divergent-grid-label-set/` | `check_h2` |
| **T1** | `time` named/typed/non-null/sorted | `invalid/non-monotonic-time/` | `check_t1` |
| **G2** | shared grid label ⇒ cell-for-cell alignment | `invalid/misaligned-shared-label/` | `check_g2` |
| **M6** | rule (b): per-basin axis strictly-increasing + interior-regular | `invalid/irregular-time-axis/` | `check_m6` |

### (b) Enforced, but no isolable on-disk negative — 7 ids

These checks **run and pass** on the valid fixture (they are real `check_*` rules,
not skips), but v0.1 cannot construct a fixture that makes **only** this check
`ran:fail` while every other check passes. The reason is code-grounded in each
case below; the **fail-path of each rule is proven by an in-memory unit test**
(a hand-built input that falsifies the rule directly, without differently-shaped
on-disk bytes).

| Id | Why no isolable on-disk negative exists (code-grounded) | Fail-path proof (in-memory test) |
|---|---|---|
| **M1** | `manifest.json` existence + valid-JSON + `format_version`-read-first is the §0 entry gate itself; a missing or non-JSON manifest is a structural `Err` (`ValidateError::ManifestUnreadable` / a parse error from `Manifest::from_json`), **not** a `conformant:false` report with M1 `ran:fail` — so there is no fixture that isolates an M1 *report* failure. M1 is the precondition for every later check, not a checkable report row. | `entry_gate_reports_unreadable_manifest_for_missing_manifest_json` (the `Err` form) |
| **I3** | I3 (`basin_id` unique) **co-trips I2** on any on-disk mutation: a duplicate `basin_id` in a per-basin `scalar_dynamic` necessarily disagrees with that basin's `basin=<id>` folder, so `check_i2` *also* fails — never I3-alone. The other place a duplicate could live — the `scalar_static` rollup — is never read for its `basin_id` *values* (the rollup's per-basin values are not surfaced as I3 input; only the per-basin in-file ids feed `in_file_basin_ids`), so a rollup duplicate yields `conformant:true`. Either way, no fixture isolates I3. | `i3_negative_on_duplicate_positive_on_distinct` (`check_i3` over a hand-built duplicate list ⇒ `ran:fail`) |
| **G1** | `Field::new` makes a **label-less gridded field unrepresentable** (a gridded `Field` carries `Some(GridLabel)` by construction — see `check_g1`'s doc), so no on-disk tree with a "positional channel axis" (a gridded field that fails to self-name) is constructible: discovery would never build such a `Field`. The rule still runs (the explicit no-positional-channel-axis check), but it cannot be made to fail on disk. | `g1_passes_only_when_every_gridded_field_self_names` (the in-memory falsifiable form) |
| **G3** | The gridded readers **error `MissingGridGeoref`** the moment a present artifact lacks georeferencing, so an on-disk no-georef tree fails **discovery** as an `Err` (`ValidateError::Discovery`) — it never reaches `build_report` to produce a `G3 ran:fail` report. `check_g3`'s falsifiable form is an empty-CRS `GridInfo`, which discovery can never build. | `data_var_without_grid_mapping_target_returns_missing_grid_georef` (the reader-side `Err`) |
| **Geo1** | The geoparquet reader **requires** `basin_id`/`delineation`/`geometry` and errors (`MissingGeometryColumn`) otherwise, and reads a single root file (recording `partitioned_by_delineation=false`). So a missing-column or partitioned outlines fails **discovery** as an `Err`, never yielding a `Geo1 ran:fail` report. (An *absent* outlines is an L1 fail; Geo1 then honestly `skipped`.) | covered by the geoparquet-reader error tests + `check_geo1`'s skip path on the L1 fixture (`missing_root_rollup_pins_exactly_l1_and_is_non_conformant`) |
| **L3** | The builder structurally NaN-fills an absent field's column over the full per-basin axis, so it cannot emit a basin that declares a `scalar_dynamic` yet materializes zero time rows — the absence-is-NaN-not-a-missing-file negative. `check_l3` runs byte-deep (each declared basin must materialize real time rows) and passes on every committed fixture; its fail-path is exercised by the in-memory/zero-row falsifiable form. | `check_l3` runs+passes on the valid fixture; the byte-deep absence-vs-NaN leg is `low3_*` reader-side coverage + the `check_l3` zero-row fail path |
| **T2** | The builder force-aligns every basin's gridded `time` axis onto the scalar axis, so it structurally cannot emit a scalar-vs-gridded mismatch — the negative needs a deliberate **on-disk corruption** of an otherwise-conformant fixture, not a builder output. `check_t2` runs byte-deep (full i64-micros axis identity) and passes on the valid fixture. | `check_t2_runs_and_fails_on_corrupted_scalar_time_column` (the on-disk corruptor) + `check_t2_runs_and_passes_on_builder_axes` |

> **Bucket arithmetic (the closed 20).** (a) 13 + (b) 7 = **20** — every §14 id
> placed in exactly one bucket, and **all 20 RUN** under HDX 0.2 (no R3 skips
> remain). The classification is the human-readable twin of the pinned golden test
> `golden_clearly_reports_which_checks_ran_vs_skipped` in
> [`crates/core/src/validate.rs`](../crates/core/src/validate.rs), which asserts
> every id `ran:pass` on the valid fixture (20/20).

### Confirmed against `validate.rs build_report`

The matrix matches what
[`crates/core/src/validate.rs`](../crates/core/src/validate.rs) `build_report`
actually runs vs skips — **confirmed by reading the source**:

- **`ran:pass` set on the valid fixture** — M1–M4 (entry-gate convention,
  `ran_pass` arm), M5, L1, L2, I1, I2, I3, H1, H2, T1, G1, G2, G3, Geo1 (each from
  its `check_*` rule returning `ran_pass`). This is the bucket-(a) checks (which
  pass on the *valid* fixture) plus the bucket-(b) checks (which pass *and have no
  isolable negative*).
- **`ran:fail`-on-its-mutation set** — exactly the **12** bucket-(a) ids, each on
  its dedicated negative (the M2/M3/M4 trio as an entry-gate `Err`, the other nine
  as a `conformant:false` report). Each is pinned by an `assert_pins_exactly` (or
  entry-gate `Err`) regression test naming the single failing id.
- **the three honest skips** — exactly **M6** (`check_m6` rule (b)), **L3**
  (`check_l3`), **T2** (`check_t2`), each `Skipped` / `ByteDeep` / non-empty
  reason. No other id is ever `Skipped` on the valid fixture (Geo1 *only* skips
  when outlines is *absent* — the L1-fail case — which is bucket (b), not a v0.1
  honest-skip leg).

So the bucket-(a) negatives, the bucket-(b) pass-with-no-isolable-negative checks,
and the bucket-(c) skips together account for `build_report`'s every outcome on
every fixture in this suite.

---

## Seeding, not enforcement

The generator ships **no Rust and enforces nothing.** The valid fixture engineers
the on-disk **preconditions** for the §14 checks; enforcement lives in `hdx-core`'s
`validate`. The table below maps each check to the property the valid fixture
provides and which generator module engineers it. This is a **seeding** claim —
read "seeds the precondition for", never "enforces".

| Check (§14) | Seeded on-disk precondition (the valid fixture provides…) | Generator module |
|---|---|---|
| **L1** | both root rollups (`scalar_static.parquet`, `outlines.geoparquet`) present | scalar + outlines |
| **L2** | every basin dir is `basin=<id>` with `scalar_dynamic.parquet` + `gridded_static/`/`gridded_dynamic/` | scalar + grids |
| **L3** | no stray/ragged files; a field's gap is NaN-filled, never a missing file (the Zarr NaN-fill) | grids |
| **I1** | `basin_id` is a real in-file column in `scalar_static`, every `scalar_dynamic`, and `outlines` | scalar + outlines |
| **I2** | in-file `basin_id` agrees with the `basin=<id>` folder for every basin | scalar |
| **I3** | `basin_id` is unique across the dataset (one rollup row per basin) | scalar |
| **H1** | every basin carries the identical field schema (same names, dtypes, quadrants) | scalar + grids |
| **H2** | the grid-label set (`{era5}`) is identical across basins | grids |
| **T1** | the scalar `time` column is named `time`, full timestamp, non-nullable, sorted ascending | scalar |
| **T2** | within each basin the `scalar_dynamic` and Zarr `gridded_dynamic` share the identical time axis; gaps NaN-filled | scalar + grids |
| **G1** | one artifact = one grid; fields self-name (COG band description / CF variable = field name); no positional channel axis | grids |
| **G2** | a shared grid label across the static/dynamic subtrees that **does** exhibit cell-for-cell alignment | grids |
| **G3** | Zarr CF georef (explicit `lat`/`lon` + `grid_mapping`); COG standard georeferencing tags | grids |
| **Geo1** | `outlines.geoparquet` rows `(basin_id, delineation, geometry)`; label column `delineation`; not partitioned | outlines |
| **M5** | the manifest `crs` (EPSG:4326) matches the CRS carried in every georeferenced file | manifest + grids |
| **M6** | the manifest `cadence` ("daily") is consistent with the realized daily `time` axes | manifest + scalar |

> The valid fixture also carries the two at-risk properties — parquet
> `time` **row-group statistics** (§8) and Zarr **consolidated metadata** (§8) —
> which §8 mandates and which the Rust readers confirm (see Rule 3).

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
  checks so the Rust readers and `validate` can read and enforce them; it enforces
  nothing itself.

Its own checks are **writer-side self-assertions** (Python), distinct from the
Rust-side enforcement in `validate`. Diagnostics in the generator go through the
standard `logging` machinery (to stderr), never raw `print`; the single
user-facing status line is *output* — mirroring the architecture §2 split between
diagnostics and output.

This closes the "generator masquerading as a writer" risk explicitly.

### Rule 2 — Derived, not hand-authored (HARD RULE)

Every invalid fixture **MUST** be
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

### Rule 3 — Rust-side confirmation hand-off

The generator's self-assertions are **Python-side**: they assert what the
*writer* intended, which cannot prove what a *Rust reader* recovers from the same
bytes. Two engineered properties are most at risk of a writer/reader mismatch:

1. **Parquet `time` row-group statistics.** pyarrow may
   or may not emit usable min/max statistics for the timestamp logical type under
   the chosen settings. The generator self-asserts the *written file* carries them
   (`assertions.assert_time_column_and_statistics`); the Rust side confirms
   (`arrow`/`parquet`) that the time extent is sourced from those
   statistics (not a bounded-scan fallback) on the valid fixture.
2. **Zarr v3 consolidated metadata.** `zarr-python`'s v3
   consolidated-metadata layout must be readable by Rust `zarrs`. The generator
   self-asserts consolidated metadata is present
   (`assertions.assert_zarr_consolidated_and_sharded`); the Rust side confirms
   that it reads the store's metadata via the §8 consolidated path
   (or explicitly classifies it an R3 byte-deep skip, with a stated reason).

> **The hand-off rule:** if the Rust reader cannot recover a property
> the generator asserted, the fix is to **REGENERATE the fixture** (adjust the
> generator and re-emit) — **never** to add a reader workaround. A mismatch is a
> generator bug, not a reader bug.

---

## Inert / agnostic discipline

`manifest.json` is **exactly** the six floor fields (spec §11): `format_version`,
`name`, `created_at`, `producer_version`, `crs`, `cadence`. No content hash, no
data-version, no field catalog, no basin list, no transform/role/semantic/
provenance key. Field names are opaque producer strings; the `{source}_{variable}`
and companion-mask `{field}_was_filled` patterns appear **only to prove the verbs
give them no special handling**. `delineation` labels are neutral, not
trusted "hydrofabric" sources. `format_version` is a **hard cut** (`"0.1"` in the
baseline).
