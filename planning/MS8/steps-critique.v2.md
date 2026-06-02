# MS8 STEP-plan critique — adversarial review

**Verdict: NOT APPROVED.** One HIGH issue (the I3 fixture is mis-specified against the
actual `validate.rs` — it cannot achieve one-violation purity and is likely a
no-negative in v0.1), plus medium/low refinements. Everything else in the plan is
unusually well-grounded: I verified the load-bearing claims directly against
`crates/core/src/validate.rs`, the readers, the generator, and the committed
fixtures/goldens, and the enforced-vs-skipped reconciliation, the HIGH-1/HIGH-2
collision proofs, the Bucket-A error variants, and the reuse-of-existing-tests claims
all check out at the cited line numbers.

---

## What was verified true (so it is not re-litigated)

These claims are LOAD-BEARING and were confirmed against the code, not assumed:

- **Bucket model (§0.1) matches `build_report` (validate.rs:1327-1396).** M1-M4 are
  entry-gate `ran_pass` (a manifest violation returns `Err` before discovery), so M3/M4
  negatives are genuinely Bucket A. L3 (validate.rs:850), M6 rule (b) (validate.rs:1070),
  and T2 (validate.rs:1090) are unconditional/dominant `Skipped` — genuinely Bucket C.
  The enforced set that can yield `conformant:false` via an on-disk one-mutation is
  exactly {M5, L1, L2, H1, H2, I2, T1(sort/null/dtype/name), G2} — the plan's Bucket B.
- **HIGH-1 (L2, S3) is real and resolved by code.** `check_l2` (validate.rs:811) fails
  when `declares_gridded_static && static_artifacts().is_empty()`. Deleting one basin's
  COG: H1 is scalar-only (`fields_by_basin`, validate.rs:1406-1416 → `BasinScalar::fields()`)
  → pass; H2 set is `{era5}` from the surviving Zarr → pass; `check_g2` (validate.rs:1114)
  iterates `static_artifacts()`, the mutated basin has none → its loop body is skipped →
  G2 pass. L2-only is achievable. Confirmed.
- **HIGH-2 (H2, S3) is real and resolved by code.** `fields_by_basin` is built from
  `discovery.scalar().per_basin()` (validate.rs:1406) and `schema_key` (validate.rs:519)
  reads `grid_label` only for the scalar fields it is given — scalar fields carry no grid
  label. A gridded relabel therefore cannot enter H1's input. `check_h2` (validate.rs:561)
  is a `BTreeSet` over static⊕dynamic labels → relabel one basin to `{chirps}` ≠ `{era5}`
  → H2-only. Confirmed; the prior "label feeds both rules" concern genuinely does not hold.
- **Bucket-A error variants are the real ones.** `read_outlines` errors
  `MissingGeometryColumn` (geoparquet_reader.rs:193-195) and hard-codes
  `partitioned_by_delineation: false` (geoparquet_reader.rs:230) — so Geo1's partition
  leg is structurally unreachable and the missing-column negative is `Err`, not a fail.
  `read_scalar_dynamic` errors `MissingScalarColumn` (discovery.rs:315); the Zarr/COG
  readers error `MissingGridGeoref` (gridded_discovery.rs:331-332). `check_g3`
  (validate.rs:1167) fails only on an empty CRS string, unreachable from disk. All
  confirmed.
- **Companion-mask/`{source}_{variable}` ordinariness** is already pinned by
  `validate_treats_companion_mask_fields_as_ordinary` (validate.rs:2086) and the describe
  golden; the fixture carries `era5_precipitation` + `era5_precipitation_was_filled`
  verbatim. S6's "re-affirm, do not re-handle" is correct.
- **All reuse/reference claims resolve.** The cited existing tests exist at the cited
  lines: `missing_root_rollup_pins_exactly_l1_and_is_non_conformant` (1950),
  `wrong_format_version_never_reports_conformant_true` (1915),
  `m3_in_memory_negative_seven_field_manifest_rejected` (1542),
  `m4_in_memory_negative_empty_crs_and_bad_created_at_rejected` (1561),
  `m6_on_valid_fixture_is_not_a_fail_and_names_the_regularity_leg` (1988),
  `entry_gate_reports_unreadable_manifest_for_missing_manifest_json` (1526). Both goldens
  (`describe.golden.json`, `validate.golden.json`) exist; the snapshot tests
  (`validate_json_equals_committed_golden`, `golden_validates_against_validate_schema`,
  `golden_clearly_reports_which_checks_ran_vs_skipped`) exist and lock the byte-unchanged
  golden — so S6's "re-affirm, not extend" is honest, not an overclaim.
- **Generator scaffolding claims resolve.** `Invalid` enum (mutate.py:47),
  `derive_invalid` (mutate.py:113), `derive_invalids` (build.py:67, called at build.py:109),
  `assert_differs_in_exactly_one_way(baseline, invalid, invalid_enum)` (assertions.py:754),
  `assert_time_column_and_statistics` (assertions.py:79, runs sortedness). S1's plan to
  ADD an `MS8Invalid` enum + a NEW generalized `assert_ms8_differs_in_exactly_one_way`
  helper (rather than break the existing two-variant hard-coded one) is the right move.
  The H2 Zarr-rename is genuinely a many-file change (era5.zarr is ~13 files); the plan's
  `removed={era5.zarr/**,...}` / `added={chirps.zarr/**,...}` table handles it.
- The valid fixture has **3 basins** (0001/0002/0003), so "mutate one basin" always leaves
  two basins as a homogeneous reference for the cross-basin checks. Good.

## Folded STEP-2 issues — all genuinely incorporated

- **Enforced-vs-skipped reconciliation (CRITICAL):** §0 reads `validate.rs` first and
  derives the three buckets from the actual code; the split is stated in §0.1 and S6's
  README. Incorporated.
- **M6 negative (MED-1):** S5 + the coverage table treat M6 as Bucket C
  (`rule (b)` skipped, dataset conformant), explicitly do NOT resurrect the cross-basin
  same-step rule, and point at the existing `m6_..._names_the_regularity_leg` test.
  Incorporated.
- **LOW-2 (programmatic one-mutation generation + the differs-in-exactly-one-way
  self-assertion):** every fixture is derived from the single baseline via the generator;
  S1 generalizes the self-assertion; S5's skip demos live in a separate `skip-demo/` dir
  with a separate README sub-table. Incorporated.
- **Goldens:** S6 wires the matrix test and re-affirms BOTH goldens byte-unchanged with
  the documented golden-update workflow; companion-mask ordinariness re-affirmed in both
  verbs. Note the *wording* nuance below (the milestone text says "extend"; the plan
  correctly downgrades to "re-affirm byte-unchanged" — that is the honest reading, see
  MED-2). Incorporated, with a wording reconciliation flagged.

---

## Issues

### HIGH — the I3 fixture (S2) is mis-specified and cannot be one-violation-pure

**This is the same class of defect the plan's own HIGH-1/HIGH-2 analysis was built to
catch — and it was missed for I3.**

`check_i3` (validate.rs:600) consumes `in_file_basin_ids` (validate.rs:1439-1446), which
is built **exclusively** from `discovery.scalar().per_basin()` → each basin's
**`scalar_dynamic`** in-file `basin_id`, taken as `table.basin_id_values().first()`
(discovery.rs:261). The root `scalar_static.parquet` contributes only (a) the static
*field schema* and (b) a `scalar_static_has_basin_id` *presence bool* (discovery.rs:175,
216) — its per-row `basin_id` **values are never surfaced** to any check.

Consequences for S2's I3 fixture as written ("rewrite the **root**
`scalar_static.parquet` so two rows carry the same `basin_id`, and one basin's
`scalar_dynamic` in-file id matched accordingly"):

1. Mutating `scalar_static.parquet` row values has **zero effect on I3** — those values
   are never read for uniqueness.
2. The only way to put a duplicate into `in_file_basin_ids` is to set **two basins'
   scalar_dynamic in-file `basin_id` to the same value**. But `check_i2` (validate.rs:934)
   runs unconditionally in `build_report` and fails the moment a basin's in-file id ≠ its
   folder id. Duplicating an id across two distinct `basin=<id>` folders forces an I2
   mismatch on at least one of them. So the only realizable I3 fixture **also trips I2**
   — a two-check collision, which violates the "exactly this id fails, others pass/skip"
   contract and is left unresolved.
3. The plan's I3 row claim "I2 stays pass (folder still agrees for the value that is
   duplicated)" is incoherent: a duplicate needs two basins sharing one in-file id, and at
   most one of the two can match its folder.

Because I2 enforces folder==in-file and I3 sees one id per basin, **I3 has no clean
one-violation `conformant:false` negative in v0.1** — exactly analogous to G1 (which the
plan correctly classifies as a documented no-negative). The plan should classify I3 the
same way, with a code-cited finding, instead of asserting a Bucket-B fixture that the
code contradicts.

*Suggested fix:* Reclassify I3 as a **no-enforceable-v0.1-negative** finding (like G1):
document in S2/README that `check_i3`'s only input is the per-basin scalar_dynamic
`.first()` in-file id, that any cross-basin duplicate necessarily trips I2 first, and that
intra-file row duplicates are invisible (discovery takes `.first()`), so no surgical
single-check I3 fixture exists. Remove the `I3_DUPLICATE_BASIN_ID` Bucket-B fixture and
its `i3_duplicate_basin_id_pins_exactly_i3` test; cover I3 only via the positive baseline
(already conformant) + the documented finding. (If the team instead wants an enforceable
I3 negative, that requires surfacing scalar_static row `basin_id` values into the I3 input
— which is a `build_report`/discovery change and is therefore OUT of MS8 scope; it must be
raised as an architecture finding, not patched here.)

### MEDIUM — S6 "matrix asserts every other id pass-or-skip" must tolerate the I3 fix

Whichever way the HIGH is resolved, the S6 consolidated matrix (and the coverage table at
the bottom of the plan, which lists I3 as Bucket B) must be updated so I3 is not a
Bucket-B row asserting `conformant:false`. As written, S6's exhaustive
"pinned id `ran:fail`, every other id pass-or-skip" loop would either (a) fail to compile
a row for a fixture that doesn't exist, or (b) green-light a fixture that trips two checks.
The coverage table row `| I3 | B | i3-duplicate-basin-id … |` is the artifact that makes
this look covered when it is not.

*Suggested fix:* after resolving the HIGH, change the I3 coverage-table row to the
no-negative form (mirroring the G1 row's footnote treatment) and ensure the S6 matrix
table contains no Bucket-B I3 entry.

### MEDIUM — milestone text says "extend the golden"; plan says "byte-unchanged / re-affirm". Reconcile explicitly.

`milestones.md` MS8 deliverables say "Golden `describe` outputs for the valid fixture(s)
(**extending** MS5's golden)". The plan (S1, S6, Scope guard) instead asserts the valid
baseline and both goldens are **byte-unchanged by the determinism contract** and only
*re-affirmed*. The plan's reading is the correct one — MS8 adds no new field and changes
no baseline bytes, so there is genuinely nothing to "extend" in the goldens; the new
golden-bearing artifact is the validate golden plus the matrix test. But this is a
visible divergence from the milestone wording, and a reviewer checking deliverable-vs-plan
literally will flag it.

*Suggested fix:* add one sentence in S6 (and the milestone-goal blockquote) explicitly
stating that "extend the golden" is satisfied by *adding the MS8 negative/skip matrix and
the documented golden-update workflow around the existing byte-unchanged goldens* — i.e.
extending the golden *suite*, not rewriting the valid-fixture golden *files* — so the
deliverable text and the plan agree on the record.

### LOW — S4 G2-misalignment fixture: pin which artifact is mutated and assert discovery-succeeds-first

`check_g2` (validate.rs:1127) compares `extent`/`resolution`/`width`/`height` between the
shared-label COG and Zarr. The S4 fixture proposes "shift the extent by one cell, or change
the pixel count" but leaves the choice open. A pixel-count change risks a reader decode
path that differs from an extent shift, and (like T1) a malformed grid could surface as a
discovery `Err` (Bucket A) rather than a G2 `ran:fail`. Mirror the T1 discipline already in
the plan: pin ONE concrete mutation and have the test assert the fixture **discovers
successfully first**, then G2 `ran:fail`.

*Suggested fix:* in S4, pin the G2 mutation to a single concrete change (e.g. shift the
Zarr `lon`/`lat` origin by one cell so `extent` differs while width/height/res are
unchanged and the store still decodes), and add `discovers OK, then G2 ran:fail` to the
named test, exactly as `t1_unsorted_time_pins_exactly_t1` does.

### LOW — H1 dtype-mutation fixture (S3): pin the dtype pair against `parse_dtype`

S3's `H1_DIVERGENT_SCALAR_SCHEMA` mutates one basin's `streamflow` dtype "(e.g. float64 →
float32)" and hedges "(if `parse_dtype` rejects the chosen physical type, pick a mapped
one)". `check_h1`'s `schema_key` (validate.rs:519) includes `dtype`, so a mapped dtype
divergence does trip H1-only — but an *unmapped* physical type would surface as
`UnknownDtype` at discovery (Bucket A), not H1. The plan already says "the test asserts
discovery succeeds first" which mitigates this; just pin the exact float64→float32 pair (or
whichever pair the closed `Dtype` enum maps) so the fixture is deterministic and not
left to implementation-time guessing.

*Suggested fix:* name the exact source/target physical type in S3 and confirm both map
through MS1's closed `Dtype` parse before coding the fixture.

---

## Scope / ordering / conventions — clean

- **Scope:** No step adds a `check_*` rule, edits `build_report`, mutates the manifest type,
  adds a domain field, or touches `regrid`/`clip`/`reduce`. The M3 negative is a 7-field
  JSON *string*, not a type change. The S4 architecture.md amendment is appended to the §8
  table (newest-first, dated) per the table's own convention, and any rule-gap is an
  explicit halting finding rather than a patch. In scope. (The I3 HIGH does **not** become
  a scope violation only because the plan's *suggested* fixture is wrong, not because it
  proposes editing rules — but note that the *correct* I3 negative would require a
  discovery change, which would be out of scope; that reinforces the no-negative
  resolution.)
- **Ordering:** S1 (scaffold, byte-preserving) → S2/S3/S4/S5 (fixtures, each independently
  green) → S6 (consolidation). No step depends on a later step. Each step's tests run only
  against fixtures that step introduces (or pre-existing ones). Buildable as written.
- **Green/committable:** every step lists `cargo build` + `cargo test` +
  `cargo clippy --all-targets -- -D warnings` and conventional `test(ms8): …` commit
  messages; the version-bump+tag convention is implied by the repo's mandatory workflow
  (the plan does not restate it per-step, which is consistent with the other milestones).
- **Conventions:** tests-only Rust additions; no `println!`, no lib `unwrap`/`expect` in
  production code (the changes are in `#[cfg(test)]` modules + Python generator), enums
  over bools is honored by the existing `CheckStatus`/`MS8Invalid` design, manifest
  extra/missing-field handling is exercised as `Err`, not silently. Clean.

---

## Coverage ledger (after the HIGH fix)

| §14 | Plan bucket | Verified outcome | Note |
|---|---|---|---|
| M1 | A (ref) | entry `Err` (MS6) | OK |
| M2 | A (ref) | entry `Err` | OK |
| M3 | A | `ExtraManifestField` `Err` | OK |
| M4 | A | `EmptyCrs`/`InvalidTimestamp` `Err` | OK |
| M5 | B | M5 `ran:fail` (manifest crs ≠ grid crs, grid loop fails first) | OK |
| L1 | B (ref) | L1 `ran:fail` (Geo1 skips on absent outlines) | OK |
| L2 | B | L2-only (HIGH-1 verified) | OK |
| H1 | B | H1-only (scalar dtype) | OK once dtype pair pinned (LOW) |
| H2 | B | H2-only (HIGH-2 verified) | OK |
| I1 | A | `MissingGeometryColumn` `Err` | OK |
| I2 | B | I2 `ran:fail`, I3 stays distinct | OK |
| **I3** | **B (WRONG)** | **no clean v0.1 negative — collides with I2** | **HIGH — reclassify as no-negative** |
| T1 | B | T1 sort leg, discovers-first | OK |
| G1 | doc | no-negative (label-less unrepresentable) | OK |
| G2 | B | G2-only | OK once mutation pinned (LOW) |
| G3 | A | `MissingGridGeoref` `Err` | OK |
| Geo1 | A | `MissingGeometryColumn` `Err`; partition always-false | OK |
| L3 | C | unconditional skip, conformant | OK |
| M6 | C | rule (b) skip, conformant | OK |
| T2 | C | unconditional skip, conformant | OK |

Every other check has a code-verified, bucket-appropriate outcome. The single gap is I3.
