# Adversarial critique — MS8-COMPLETION step plan (`steps-completion.md`)

Reviewed against: `spec/HDX_SPEC.md` §14, `architecture.md`, `conformance/README.md`,
`crates/core/src/validate.rs` (the frozen ran-vs-skip ground truth), the generator
(`mutate.py`, `assertions.py`, `build.py`, `scalar.py`, `manifest.py`,
`gridded_discovery.rs`), and the on-disk `.gitignore` + tracked file set.

**Verdict: APPROVED.** No high/critical issues. The plan's core technical reasoning
(a)–(f) is correct and verified against the frozen `validate.rs`. The remaining issues
are executable-precision / completeness gaps in Step 1, all of which fail loudly at the
`cargo test` acceptance gate rather than producing a silently-wrong result. Highest
severity = **medium**.

---

## (a) Does Step 1 actually relocate goldens OUT of the gitignored trees and prove regenerate no longer clobbers them? — YES (verified)

The diagnosis is exactly right, confirmed on disk:

- `.gitignore` ignores `conformance/valid/**` + `conformance/invalid/**`, re-includes
  `!conformance/**/*.golden.json`. All 7 tracked goldens (`git ls-files | grep golden`)
  physically live inside the two gitignored trees.
- `mutate._copy_baseline` (line 224) does `shutil.rmtree(target_root)` on each invalid
  target **before** `copytree(..., ignore=ignore_patterns("*.golden.json"))`. The `ignore`
  only prevents copying the *baseline's* golden into the target; it does **not** protect a
  pre-existing tracked golden already sitting in the target tree — that golden is destroyed
  by the `rmtree`. The plan's claim ("the ignore only protects the baseline copy, not the
  pre-existing target tree contents that `rmtree` wipes") is precisely correct.
- `conformance/goldens/` is verified NOT gitignored (`git check-ignore` returns
  not-ignored), and it sits outside both `valid/`/`invalid/` trees, so neither the
  `.gitignore` rules nor any `rmtree` in the generator touch it. Relocating there genuinely
  fixes the clobber.

This is the right fix and it is the correct first step.

## (b) Is any fixture DATA committed? — NO (correctly forbidden)

The plan's scope guard is explicit: trees stay gitignored, only generator source,
relocated `goldens/*.json`, README, Rust tests, `.gitignore`, `architecture.md`, and
`Cargo.toml`/lock are committed. Goldens are small JSON assertion baselines produced by
the Rust verb, not binary fixture blobs. Consistent with the tracking policy. No issue.

## (c) Does each M5 / G2 / H2 negative pin EXACTLY one enforced check? — YES (verified against rule code, no co-trip)

- **M5 (`crs-mismatch`, manifest crs → EPSG:3857, files keep 4326).** `check_m5`
  (validate.rs:970) is the **only** rule that reads `manifest.crs()`. The grids leg
  short-circuits `ran:fail` on the first grid. M4 still passes (`EPSG:3857` non-empty); G3
  reads the *file* CRS (unchanged, non-empty) → pass; M6 reads cadence, not crs. No second
  check trips. On the valid golden M5 is `ran:pass`, which confirms the outlines leg
  resolves `EpsgFromProjjsonId` (a `RawProjjsonR3` would have made M5 `skipped`), so the
  outlines leg is also under the same id M5 — but the grids leg fires first regardless.
  **M5-only. Sound.**

- **H2 (`divergent-grid-label-set`, one basin's COG+Zarr renamed era5 → era5b).** Verified
  the rename does NOT co-trip H1, G1, G2, or L2:
  - `check_h2` uses `labels_by_basin` (per-basin static⊕dynamic label sets): the renamed
    basin's set `{era5b}` ≠ reference `{era5}` → `ran:fail`. ✓
  - `check_h1`'s `fields_by_basin` (validate.rs:1409) is built from
    `discovery.scalar().per_basin()` — **scalar fields only**. Gridded fields and their
    grid labels are **not** in the per-basin H1 key, so the rename cannot trip H1.
  - `gridded_fields()` (the dataset-wide catalog G1 reads) is assembled from the **first**
    basin (`assemble_gridded_field_catalog`, gridded_discovery.rs:394). The catalog field
    still carries `Some(GridLabel)` (era5 from basin 0001), so G1 passes.
  - G2 passes **because both** the COG and Zarr are renamed identically: within the renamed
    basin the shared label `era5b` still has aligned COG+Zarr geometry. The plan's
    requirement to rename *both* artifacts is load-bearing and correctly stated.
  - **Caveat to honor at implementation:** the rename MUST be applied to a **non-first**
    basin (not `basin=0001`), and BOTH `era5.tif`→`era5b.tif` and `era5.zarr`→`era5b.zarr`
    MUST be renamed in lockstep. Renaming only one artifact would split that basin's
    static/dynamic labels and could perturb G2; renaming the first basin would change the
    one-basin catalog. The plan says "one basin's COG+Zarr" (both) — correct — but does not
    pin *which* basin nor warn against the first-basin catalog interaction. See issue MS8C-3.

- **G2 (`misaligned-shared-label`, one basin's COG geometry shifted/scaled).** `check_g2`
  (validate.rs:1113) fires only for a label present in BOTH subtrees of a basin and then
  requires extent/resolution/width/height equality. A geometry shift on one basin's COG
  diverges from that basin's Zarr → `ran:fail`. No co-trip: M5 (CRS unchanged) pass; G3
  (CRS non-empty) pass; H2 (label still era5) pass; L2/L3 unaffected. The catalog field is
  geometry-independent (band name + dtype + label only), so H1/G1 unaffected **provided**
  the shift is not applied to the first basin's representative read — same first-basin
  caveat as H2. **G2-only. Sound.**

No repeat of the I3/I2-coupling class of defect: each mutation is traced to a single rule
and the co-trip surface is enumerated and clear.

## (d) Is M6 correctly handled as still-conformant / skipped? — YES (no resurrected cross-basin rule)

`check_m6` (validate.rs:1058): rule (a) cadence-non-empty `ran`/passes; rule (b) per-basin
axis REGULARITY is honest R3 `Skipped` (`ByteDeep`) — it never fails, never interprets the
cadence word, and asserts **no** cross-basin step equality. An irregular per-basin time
axis is therefore: T1 still passes (irregular ≠ non-monotonic; still sorted/non-null/
timestamp/named time), M6 stays `skipped`, dataset stays `conformant:true`. The plan
classifies S3 exactly as still-conformant/skipped (NOT a `conformant:false` negative) and
explicitly states "No enforceable M6 negative exists in v0.1" and asserts no cross-basin
rule. **Correct and faithful to the frozen rule.**

## (e) Does the README matrix classify all 20 checks correctly vs what validate runs/skips? — GROUND TRUTH CORRECT; matrix itself is deferred to S4

The plan's §0 ground-truth (lines 44–51) matches `validate.rs` exactly: on the valid
fixture, `ran:pass` for M1–M5, L1, L2, I1, I2, I3, H1, H2, T1, G1, G2, G3, Geo1; honest
`skipped` for **M6, L3, T2** (the three `ByteDeep` legs), with M1–M4 as the entry-gate
`ran:pass` convention. Confirmed against the committed valid golden (M5 ran:pass; M6/L3/T2
skipped) and against `golden_clearly_reports_which_checks_ran_vs_skipped`
(validate.rs:2262, which pins exactly `{M6, L3, T2}` as the skip set). The S4 matrix is not
yet written, so its literal content can't be verified — but it is built on a correct
ground-truth foundation. No issue beyond the deferral.

## (f) Any hdx-core reader / rule / domain change? — NO

The scope guard freezes readers, rule functions, domain types, manifest floor, and report/
describe wire shapes. The only `crates/core/src/*.rs` edits the plan implies are to the
**`#[cfg(test)]` modules** (repointing golden path strings). Test-helper path strings are
not reader/rule/domain/floor logic, so the freeze is honored. Worth stating explicitly in
the plan (it is implied, not spelled out) — see issue MS8C-2.

## (g) Independently green, committable, ordering sound? — Mostly; ordering is correct

Ordering rationale is sound: S1 (relocation) must precede S2/S3 because their
`regenerate.sh; cargo test` gate is unreliable while goldens still live where regenerate
writes; S4 last because it must describe the complete S2/S3 fixture set. S2 and S3 are
correctly independent. Version-bump + tag discipline is restated. The acceptance gate
(`regenerate.sh` then build/test/clippy, goldens survive regenerate) is the right gate and
will catch the precision gaps below loudly (compile/test failure), not silently.

---

## Issues

### MS8C-1 (medium) — Step 1 undercounts the Rust path-construction sites ("both Rust test helpers")

There are **three** golden-path construction sites across **two** files, not "both
helpers":
1. `crates/core/src/describe.rs:1127` — `conformance("valid/minimal/describe.golden.json")`
   (the `describe` `golden_value` helper);
2. `crates/core/src/validate.rs:2197` — `conformance("valid/minimal/validate.golden.json")`
   (the `validate` `golden_value` helper);
3. `crates/core/src/validate.rs:2414` — `conformance(name).join("validate.golden.json")`
   (the `fixture_golden_value` helper used by all 5 committed Bucket-B invalids **and** the
   3 new S2 invalids). This site does not just need a path change — its **naming scheme**
   must be rewritten from `invalid/<name>/validate.golden.json` to the new flattened
   `goldens/invalid-<name>.validate.json` (the plan's own scheme table). A literal
   `conformance(name).join(...)` cannot express the flattened name; the helper must map the
   fixture path to the flattened golden filename.

If the planner repoints only "both" (the two `valid/minimal` helpers) and misses #3 or its
naming rewrite, the 5 existing invalid-golden tests and the 3 new S2 invalids will fail to
resolve their goldens. This is caught at the `cargo test` gate (loud failure), so it is not
critical — but the plan's wording is imprecise about the highest-volume site.

**Suggested fix:** Enumerate all three sites explicitly in Step 1, and specify the
`fixture_golden_value` rewrite to the flattened `goldens/invalid-<name>.validate.json`
scheme (a fixture-name → golden-filename mapping helper), not a path-join.

### MS8C-2 (medium) — The "structured object" the plan defers Step 1 detail to does not exist

Step 1 (lines 95–103) and Step 2 (line 97) say "(Detailed in the structured object;
summary here.)" but no structured object is present in `planning/MS8/`
(`steps-completion.md` and `steps-completion.v1.md` are byte-identical 8225-byte summaries;
there is no JSON/YAML/structured step artifact). The highest-stakes step (the relocation
that unblocks everything) is left with only a one-paragraph prose summary and no concrete,
reviewable file-operation list (which `git mv`, which `.gitignore` lines removed, which doc
references updated). An executor has no authoritative checklist for S1.

**Suggested fix:** Either inline the full Step 1 file-operation list into the markdown (the
7 `git mv` targets, the `.gitignore` edit, the 3 Rust sites, the README/workflow edits, the
generator hack-strip in `mutate._copy_baseline` + `assertions._relative_files`), or attach
the actual structured object the prose references.

### MS8C-3 (medium) — S2 fixtures need a pinned target basin + first-basin-catalog warning, and a per-fixture `assert_differs_in_exactly_one_way` clause

`assemble_gridded_field_catalog` reads the **first** basin as the representative
(gridded_discovery.rs:401/411). The H2 rename and the G2 geometry shift MUST be applied to
a **non-first** basin (e.g. `basin=0002`), or the one-basin catalog read changes and could
perturb G1/H1. The plan says "one basin" without pinning which or warning of the first-
basin coupling. Additionally, each of the 3 new fixtures needs its own per-fixture clause
in `assertions.assert_differs_in_exactly_one_way` (mutate.py + the `Invalid` enum + the
assertions `match`), which the plan references only generically. The G2 fixture in
particular changes raster bytes inside `gridded_static/<label>.tif`, so the one-mutation
self-assertion's "exactly one changed file" clause must be authored for a binary-diff file,
not just a manifest/parquet diff.

**Suggested fix:** Pin the mutated basin to a non-first basin for both H2 and G2; add a
sentence noting the first-basin catalog coupling; specify the new `Invalid` enum members,
`_mutate_*` functions, and the per-fixture `assert_differs_in_exactly_one_way` clauses.

### MS8C-4 (low) — S3's M6 fixture does not fit the `Invalid`-enum generator structure, and is unspecified

`build.py` emits exactly one valid baseline (`valid/minimal/`) plus a loop over the
`Invalid` enum (`derive_invalids`). The M6 still-conformant fixture is a **second valid-
shaped** fixture (irregular time axis, `conformant:true`), so it is NOT an `Invalid` and
cannot be derived by that loop — it needs its own emit path (or a non-Invalid valid-variant
mechanism) in `build.py`. The plan does not say how this fixture is generated. Note also
its `validate.golden.json` would be **content-identical** to the baseline's validate golden
(the report encodes no time values; M6 stays `skipped`, conformant `true`), so the golden
is a near-duplicate whose value is exercising the discovery path on an irregular axis — the
plan should state this so the snapshot is not mistaken for dead weight.

**Suggested fix:** Specify the M6 fixture's emit path in `build.py` (a second valid
fixture, not an `Invalid`), confirm it lands under `conformance/valid/<name>/` (gitignored)
with its golden relocated to `goldens/valid-<name>.validate.json`, and note the golden's
expected equivalence to the baseline validate report.

### MS8C-5 (low) — Stale crate-README golden reference + README double-edit overlap not reconciled

`crates/core/README.md:324` references `conformance/valid/minimal/validate.golden.json` —
the plan does not list the crate README among files it updates, leaving a stale path.
Separately, Step 1 says it updates "the README golden paths/workflow" while Step 4
completes the `conformance/README.md` matrix; the many existing golden-path references in
`conformance/README.md` (lines 18, 64, 76–77, 129–169, 199–206) are edited across two
steps with no statement of which step owns which lines, risking a merge/overwrite conflict.

**Suggested fix:** Add `crates/core/README.md` to the Step 1 path-update list; state
explicitly that Step 1 owns the `conformance/README.md` *path/workflow* references and
Step 4 owns the *matrix section*, so the two edits don't collide.

---

## Bottom line

The plan correctly and completely closes MS8 under the fixtures-gitignored + golden-
relocation policy. The four technical pillars an adversary would attack — relocation
actually defeats the rmtree clobber (a), no committed data (b), one-check-per-negative with
no co-trip (c), M6 still-conformant/skipped with no resurrected cross-basin rule (d), and
hdx-core frozen (f) — are all verified sound against the frozen `validate.rs` and the
generator. The five issues are precision/completeness gaps (undercounted path sites, the
absent structured object, unpinned target basin + missing self-assertion clauses, the M6
fixture's generator path, a stale doc reference), all of which surface as loud failures at
the existing `regenerate.sh; cargo test; clippy` acceptance gate rather than as silent
incorrectness. None rises to high/critical. **Approved.**
