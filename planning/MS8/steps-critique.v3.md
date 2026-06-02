# MS8 STEP-plan critique (adversarial review)

**Verdict: NOT APPROVED.** Highest severity: **HIGH**.

The plan is unusually well-grounded: it reads the real `crates/core/src/validate.rs`,
classifies every §14 id into a three-bucket split, and the I3 reclassification (the
self-described "HIGH fix") is genuinely correct against the code. The STEP-2 folds for
M6 (MED-1), LOW-2 (programmatic one-mutation), and the byte-unchanged-baseline golden
reconciliation are all incorporated faithfully. **But the same enforced-vs-skipped
reconciliation that grounds the milestone misclassifies I1 as a no-enforceable-negative
finding when the actual validator supports a clean one-violation I1 `conformant:false`
negative.** That is both a coverage gap against the MS8 deliverable ("one invalid dataset
per applicable §14 check") and a mis-grounding against `validate.rs` — exactly the failure
mode the CRITICAL fold exists to prevent. It must be fixed before approval.

---

## HIGH issues

### H-1 — I1 is mis-classified as a no-enforceable-v0.1-negative; the code supports a clean Bucket-B I1 negative (spec-drift / missing-coverage)

The plan files I1 as a Bucket-C "finding, no tree" (steps.md lines 92, 209, 629 and the S6
matrix) on the rationale that *the geoparquet reader requires all three columns and `Err`s,
and the scalar-dynamic I1 leg is gated behind L2/I2*. That rationale is **incomplete against
`check_i1` (validate.rs:879-918)**, which has **three** legs, and the plan only argues two:

1. **The `scalar_static` leg (validate.rs:881-887)** fires `ran:fail` when
   `discovery.scalar().scalar_static_has_basin_id() == Some(false)` — i.e. a
   `scalar_static.parquet` that is **present and readable but lacks the `basin_id` column**.
   I verified `read_scalar_static` (scalar_reader.rs:451-468) does **not** require `basin_id`:
   it records `has_basin_id` as a plain bool via `schema_has_basin_id` and does **not** `Err`
   when the column is absent. `discover` (gridded_discovery.rs:617-623) is just
   `discover_scalar + discover_gridded` and adds no `basin_id` requirement on the rollup. So a
   fixture that drops the `basin_id` column from `scalar_static.parquet` **discovers cleanly**
   (no `ValidateError::Discovery`) and yields `scalar_static_has_basin_id() == Some(false)` →
   `check_i1` `ran:fail` → `conformant:false`. This is a realizable, one-surgical-mutation
   Bucket-B negative.

2. **The per-basin `scalar_dynamic` leg (validate.rs:890-903)** fires `ran:fail` when a basin
   has `time().is_some()` (scalar_dynamic present) **but** `basin_id_in_file().is_none()`.
   `read_scalar_dynamic` (scalar_reader.rs:490-544) requires only `time`
   (`MissingScalarColumn`), **not** `basin_id`; an absent `basin_id` yields `has_basin_id=false`
   + `basin_id_values=Vec::new()` → `basin_id_in_file == None`, with `time` still `Some`. So
   dropping `basin_id` from **one** basin's `scalar_dynamic.parquet` also produces a clean I1
   `ran:fail`. (This leg is *not* "gated behind L2/I2": L2 only fails on a missing scalar_dynamic
   or missing gridded subtree, and I2 *skips* a basin with no in-file id, so neither co-fails.)

**Collision purity is plausible for both forms** (must be empirically pinned, like the others):
H1 reads the per-basin *dynamic-scalar* field list (`fields_by_basin`, validate.rs:1406, sourced
from `BasinScalar::fields`) and `basin_id` is never a `Field` (filtered at scalar_reader.rs:383),
so dropping the column does not change any basin's schema; I3 reads `in_file_basin_ids`
(filter_map over `basin_id_in_file`, validate.rs:1439-1446) and a dropped id simply removes that
basin's entry (no duplicate); I2 skips a `None` in-file id (validate.rs:936-938). The
scalar-dynamic-drop form is the cleaner one-mutation (`basin=0002/scalar_dynamic.parquet` minus
the `basin_id` column).

**Why this is HIGH, not MEDIUM:** the milestone deliverable is "one invalid dataset per *applicable*
§14 check, each pinning exactly one violated check id" (milestones.md MS8 deliverables + exit criteria;
coverage table row I1 = "MS8"). I1 **is** applicable — it is an enforced check that can `ran:fail` on
disk. Dropping it to a documented finding leaves the §14 fail-closed proof with a real hole and
contradicts the STEP-2 CRITICAL fold's demand that the bucket split be the EXACT set the code runs. The
plan's own discipline (it correctly caught the I3 false-positive) was simply not applied symmetrically
to I1.

**Suggested fix:** Reclassify I1 to **Bucket-B**. Add an `invalid/missing-basin-id-column/` fixture that
deletes the `basin_id` column from one basin's `scalar_dynamic.parquet` (preferred) or from the root
`scalar_static.parquet`, with an `i1_missing_basin_id_pins_exactly_i1` test asserting `!conformant()`,
I1 `ran:fail`, all others `ran:pass`-or-`skipped`, plus a committed `validate.golden.json`. Place it in
S3 (scalar-side). Update the S6 matrix (I1 row → Bucket-B), the ordering rationale, the Bucket lists at
lines 92/205-213, and the coverage table. **Empirically verify** which of the two drop-forms is provably
I1-only before committing (the same purity discipline already applied to L2/H2/G2). If — and only if — the
empirical check shows discovery actually `Err`s on the chosen drop (it should not, per the code above), the
finding stands and must say so with the observed `Err`; assumption is not allowed for the very check the
fold says to verify in code.

---

## MEDIUM issues

### M-1 — No `describe` golden is added for any invalid fixture, yet the deliverable names "Golden describe outputs … (extending MS5's golden)" (spec-drift / coverage, milestone wording)

The MS8 deliverable explicitly reads "Golden `describe` outputs for the valid fixture(s)
(**extending MS5's golden**)" and the goldens fold says "extend the committed golden outputs —
**BOTH** the describe golden (MS5) and the validate golden (MS6)". The plan extends the **validate**
golden suite (one negative report per Bucket-B invalid, S3/S4) and re-affirms **both** byte-unchanged
baseline goldens (S1), but adds **no new `describe` golden** anywhere. The plan's reconciliation
(steps.md lines 14-26) argues the baseline describe golden stays byte-unchanged and the suite is
"extended" by adding validate negatives — which is reasonable for Bucket-A (those `Err` before any
Description exists) and for Bucket-C. **But every Bucket-B invalid discovers cleanly** (only `validate`
fails; `describe` succeeds and yields a full `Description` that differs from the baseline, e.g. the
ragged-schema or divergent-grid-label tree). So a `describe` golden for at least one Bucket-B/C invalid
is squarely within the literal deliverable and would catch describe-side regressions the validate goldens
do not. Re-affirming only the *valid* describe golden does not "extend the describe golden suite."

**Suggested fix:** Either (a) add a committed `describe.golden.json` for at least the Bucket-C
`irregular-time-axis` fixture and one Bucket-B scalar invalid (asserting describe still emits a valid,
schema-conformant Description while validate fails) and wire snapshot tests; or (b) if the planner
maintains that the describe-golden deliverable is fully met by re-affirming the byte-unchanged MS5 baseline,
state that explicitly in the plan body and the S6 README with the reasoning, so a reviewer sees the literal
deliverable was considered and consciously satisfied by re-affirmation, not silently narrowed.

### M-2 — Geo1 conditional has no committed fallback acceptance, so S4 can land "green but empty" against an unmet deliverable (vague-acceptance / not-green-risk)

S4 makes Geo1 conditional on an empirical probe of `geoparquet_reader.rs`. I confirmed the reader
**hard-codes** `partitioned_by_delineation` to whatever it was constructed with and its doc says
"**Always `false`** for the single root `outlines.geoparquet` this reader is handed"
(geoparquet_reader.rs:143-149); `read_outlines` (line 181) never sets it true. So the probe will
almost certainly resolve to "downgrade Geo1 to a finding." That is a legitimate in-scope outcome
(no Rust change), **but** the plan never states the *committed acceptance* for the downgrade path:
S4's acceptance still reads "Three (or four …) new invalid trees" and "Geo1 either negative (fixture)
or a recorded finding" without pinning where the finding lands or what test guards it. As written, S4
could commit with Geo1 neither a fixture nor a documented finding and still claim green.

**Suggested fix:** In S4 acceptance, make the downgrade path concrete: "if the probe shows the reader
cannot represent a partitioned outlines, S4 records Geo1 as a no-v0.1-negative finding **in
`conformance/README.md`'s check-id matrix** and adds a `geo1_partitioned_outlines_is_not_representable_in_v0_1`
test (or a documented assertion) capturing the probe outcome; the consolidated S6 matrix lists Geo1 as a
finding." State the probe's expected result given geoparquet_reader.rs:147 so the reviewer knows the
likely branch up front.

### M-3 — L2 step bakes in an unresolved empirical branch as the committed plan, with a dangling sentence (ordering / not-green-risk / clarity)

S3's L2 mutation (steps.md lines 145-174, 382-389) is written as "delete `gridded_dynamic/`; verify
one-violation purity empirically and, if H2/H1 co-fails, fall back to the alternative deletion the
regression proves L2-only." I verified the plan's H2-collision reasoning is correct against the code
(`labels_by_basin` unions static+dynamic, validate.rs:1428; deleting `gridded_dynamic/` leaves the basin's
label set `{era5}` from the surviving COG, so H2 passes; `declares_gridded_dynamic` stays true because
`gridded_fields()` is dataset-wide, so `check_l2`'s dynamic leg fires at validate.rs:821). So the pinned
form is almost certainly fine. The issue is **process, not correctness**: a committable step should pin a
single mutation with a deterministic expected outcome, not commit a branch ("whichever the regression proves").
Lines 156-160 even contain a half-finished sentence ("...keeps the `gridded_dynamic/` directory empty is NOT
valid") that reads as unresolved drafting and should not survive into the plan of record.

**Suggested fix:** Pin the single mutation (delete one basin's `gridded_dynamic/` subtree, per the verified
H2-collision caveat) as the committed form, and demote the alternative to "rejected alternative recorded in
a code comment + README" (which the plan already does for the scalar-deletion form). Keep the empirical
purity *assertion* in the test, but remove the "fall back to whichever deletion the regression proves" as the
*plan of record* — the step must be deterministic. Clean up the dangling sentence at lines 156-160.

---

## LOW issues

### L-1 — `build_report` runs M1–M4 as `ran:pass` unconditionally; the Bucket-A tests never exercise that path (convention / clarity)

For the Bucket-A trees (M2/M3/M4) `validate` returns `Err` at the entry gate
(`Manifest::from_json` with `?`, validate.rs:1275) before `build_report` ever runs, so the
`CheckId::M1|M2|M3|M4 => ran_pass` arm (validate.rs:1373-1375) is never reached on those trees.
The plan's S2 tests assert the `Err` form, which is correct. No action required beyond a one-line note
in the S2/S6 README that M1–M4 appearing as `ran:pass` in the *valid*-fixture golden is the
already-cleared-at-the-gate convention, so a reader does not mistake it for a second enforcement site.

### L-2 — M6 `check_m6` rule (a) empty-cadence fail leg is dead code on the validate path; the plan correctly avoids claiming it but should say why (clarity)

`check_m6` rule (a) (validate.rs:1060-1066) would `ran:fail` on an empty cadence, but an empty
`cadence` is rejected by M4 at the entry gate (manifest.rs:20 — "`crs`/`cadence` are non-empty"),
so `validate(empty-cadence)` returns `Err(Manifest(EmptyCadence))` before `build_report`. The plan's
classification (M4 = Bucket-A `Err`; M6 = Bucket-C reported-skip) is therefore correct and consistent —
the empty-cadence fixture pins M4, not M6. Worth one sentence in S5/README noting that M6 rule (a) is
unreachable-by-construction on the validate path so no one later "adds an M6 empty-cadence negative" by mistake.

---

## What the plan gets right (verified against code)

- **I3 reclassification (the self-described HIGH fix) is correct.** `check_i3` (validate.rs:600) consumes
  `in_file_basin_ids` (1439-1446), each entry sourced from `table.basin_id_values().first()` (discovery.rs:261).
  Two basins sharing a first in-file id necessarily trip `check_i2` (934, runs unconditionally) since at most one
  matches its folder. So I3 has no clean one-violation negative — confirmed.
- **G1 / G3 findings are sound.** `Field::new` (field.rs:246-271) returns `Err(MismatchedGridLabel)` for a
  label-less gridded field, and the readers self-name, so a G1 `ran:fail` is unrepresentable on disk; G3's
  empty-CRS fail form (validate.rs:1166-1177) needs a `GridInfo` discovery cannot produce (readers `Err`
  `MissingGridGeoref`). Both are genuine no-disk-negative cases.
- **L3 and T2 are unconditional `Skipped` (ByteDeep)** (validate.rs:850-859, 1090-1099) — no `conformant:false`
  negative exists; the plan correctly files them as findings with no tree.
- **M2/M3/M4 are entry-gate `Err`** (manifest.rs M1–M4 table; validate.rs:1275 `?`), correctly asserted as
  `Err` outcomes, not reports.
- **M6 (MED-1 fold) is correct** — `check_m6` rule (b) is `ByteDeep`-skipped (1070-1077); the
  `irregular-time-axis` fixture's expected outcome is reported-skip + still conformant, no resurrection of the
  dropped cross-basin same-step rule. Matches the fold verbatim.
- **LOW-2 fold honored** — every invalid is a programmatic one-mutation off the single valid baseline via the
  existing generator (`mutate.py` `Invalid` enum, `assert_differs_in_exactly_one_way`), with the
  "exactly-this-id-fails" purity assertion guarding cross-contamination.
- **Golden baseline byte-unchanged reconciliation is sound** — the four committed snapshot tests
  (`validate_json_equals_committed_golden`, `golden_validates_against_validate_schema`,
  `describe_json_equals_committed_golden`, `golden_validates_against_describe_schema`) exist and lock the
  baseline; MS8 adds no field, so they stay byte-unchanged. Companion-mask / `{source}_{variable}` ordinariness
  re-assertion (S6) extends existing `validate_treats_companion_mask_fields_as_ordinary` /
  `golden_companion_mask_and_source_variable_fields_are_ordinary` tests.
- **Scope is clean** — fixtures + tests + docs only; no regrid/clip/reduce, no manifest-floor mutation
  (the M3 7th key is *rejected*, never carried), no new domain field, no reader/`build_report` change, MS7/MS9
  untouched. Bump+tag + conventional commits present on every step.
- **Ordering S1→S2→S3→S4→S5→S6 is buildable** and each step is independently green and committable (modulo
  the H-1 fix and M-3 determinism cleanup).

---

## Required before approval

1. **H-1**: reclassify I1 to Bucket-B, add the `missing-basin-id-column` fixture + pinned test + golden in S3,
   and update the S6 matrix / bucket lists / coverage table / ordering rationale. Empirically confirm the
   chosen drop-form discovers cleanly and is I1-only.
2. **M-1**: add a `describe` golden for at least one Bucket-B/C invalid (or explicitly justify in the plan +
   README that re-affirming the byte-unchanged MS5 baseline satisfies the describe-golden deliverable).
3. **M-2 / M-3**: pin the L2 and Geo1 steps to deterministic committed mutations with concrete fallback
   acceptance, and clean the dangling L2 prose.
