# MS8 STEP plan — adversarial critique

**Verdict: REQUEST CHANGES (not approved).** Highest severity: **HIGH**.

The plan is unusually well-grounded: it read `crates/core/src/validate.rs` directly,
classified every §14 check into the three real outcome buckets (A `Err`, B `ran:fail`,
C R3-skip), and all four mandated STEP-2 fold-ins are genuinely incorporated (not
cosmetic). Scope discipline is clean — no regrid/clip/reduce, no manifest mutation
beyond the M3-rejection-demo, no new domain field, no MS9 work. Ordering is sound and
each step is plausibly green.

It is **blocked by two concrete one-violation-purity defects** that the plan promises to
guard but, as written, would trip a *second* check and fail their own
"exactly-this-id-fails" assertion — leaving the tree red. These are surgical-purity bugs
in specific fixtures (L2, and a real risk for H2), grounded in the actual discovery/rule
code. They are fixable with tighter mutations, but the plan currently asserts purity it
cannot deliver, so the steps are not independently green as written.

---

## Verification of the four mandated STEP-2 fold-ins

| Fold-in | Present? | Evidence |
|---|---|---|
| **Enforced-vs-skipped reconciliation read from `validate.rs`** | **YES, genuine** | §0 buckets every check from the actual `build_report` + `check_*` functions. The Bucket-A set (M2/M3/M4 via `Manifest::from_json`; I1-outlines, G3, Geo1-missing-column via discovery `Err`) matches the code: `read_outlines` hard-`require_column`s `basin_id`/`delineation`/`geometry` (geoparquet_reader.rs:193-195) so those are genuinely `Err`, not `ran:fail`. The Bucket-B set matches the rule fns. The enforced/skipped split (§0.3) names the three skips correctly. |
| **M6 negative per MED-1** | **YES, genuine** | §0.1 Bucket-C row + S5 assert "M6 reported skipped-with-reason, still conformant," mirroring the live `m6_on_valid_fixture_is_not_a_fail_and_names_the_regularity_leg` test; explicitly forbids resurrecting the cross-basin same-step rule. Matches `check_m6` (validate.rs:1057-1078). |
| **LOW-2 programmatic one-mutation generator + self-assertion** | **YES** | S1 generalizes `assert_differs_in_exactly_one_way` (currently hardcoded per-variant, assertions.py:754) and the `Invalid` enum (mutate.py:47); §2 reaffirms "never hand-edited." Grounded in the real generator (`derive_invalids` loops `for invalid in Invalid`, build.py:79). |
| **Goldens (describe MS5 + validate MS6) + ordinariness in both verbs** | **YES** | S6 extends/re-affirms both committed goldens and the ordinariness assertions. Both goldens + snapshot/schema/ordinariness tests already exist (describe.rs `golden_companion_mask_and_source_variable_fields_are_ordinary`, validate.rs `validate_treats_companion_mask_fields_as_ordinary`). |

All four are incorporated substantively. The blocking issues below are *execution* defects
in the fixture mutations, not fold-in gaps.

---

## HIGH severity

### H-1 (not-green / missing-coverage) — S3's L2 fixture trips H1 too, breaking one-violation purity

**Step:** MS8-S3. **The defect is concrete and grounded in the code.**

S3's L2 fixture deletes one basin's `scalar_dynamic.parquet` ("forward leg `check_l2`
enforces") and asserts "`L2` `ran:fail`, all others pass/skip."

But `discover_basin` (discovery.rs:241-253) records an absent `scalar_dynamic.parquet` as
a `BasinScalar` with **`fields: Vec::new()`**. `build_report` feeds every per-basin entry
(including this empty-fields basin) into `fields_by_basin` (validate.rs:1406-1416), and
`check_h1` (validate.rs:502-551) compares each basin's `schema_key` against the first
basin's. The deleted basin's **empty** field list differs from the others' non-empty list
⇒ **H1 also records `ran:fail`** ("ragged schema").

So the L2 fixture as designed yields **two** `ran:fail` outcomes (L2 *and* H1), and the
plan's own one-violation-purity assertion ("only L2 may fail; every other check is
pass/skip") would **fail the test** — the tree is red, not green.

The plan's S3 "Surgical-purity care" paragraph anticipates an H2-vs-H1 collision and an
"L2 gridded leg twice" collision, but **does not mention the L2-vs-H1 collision**, which
is the actual one. Note that `T1` and `I1` are *not* tripped (T1 skips a `None` time
descriptor; I1's dynamic leg is gated on `time().is_some()`), so H1 is the specific second
failure.

**Suggested fix:** Either (a) re-scope the L2 fixture so it does not zero out a basin's
schema — e.g. exercise the gridded-subtree forward leg instead (declare gridded fields but
remove one basin's `gridded_dynamic/` directory while keeping `scalar_dynamic.parquet`
intact, so only `check_l2`'s gridded leg fails and H1/H2 stay homogeneous *if* the label
set is preserved — verify against `check_l2` legs at validate.rs:810-831 and `check_h2`);
or (b) explicitly accept that the L2-via-scalar-deletion fixture trips H1 and reclassify
its assertion (which contradicts the milestone's "exactly one check"). Option (a) is the
only one consistent with the stated contract. Whichever is chosen, the plan must name the
L2↔H1 interaction and prove the chosen mutation isolates exactly one `ran:fail`.

### H-2 (not-green / vague-acceptance) — S3's H2 fixture (rename one basin's `era5`→`chirps`) also trips L2 and risks G2; the plan's hedge is non-deterministic

**Step:** MS8-S3.

S3's H2 fixture renames one basin's `era5.tif`/`era5.zarr` to a different label. The plan
flags the H2↔H1 risk but not these two harder ones:

1. **L2 collision.** `check_l2`'s gridded legs require, *iff the field schema declares
   `GriddedStatic`/`GriddedDynamic` fields*, that **every** basin expose a
   `gridded_static/`/`gridded_dynamic/` artifact (validate.rs:810-831). Renaming the COG/Zarr
   filename does not by itself remove the subtree, so L2 likely still passes — **but** the
   rename changes the discovered `GridLabel`, and `check_h2` (validate.rs:561-591) compares
   the *label set* per basin: `{chirps}` vs `{era5}` ⇒ H2 fails (intended). The risk is that
   the renamed-label basin's gridded **field catalog** (the `Field`'s `grid_label`) now
   diverges, which `check_h1`'s `schema_key` includes `grid_label` (validate.rs:519) ⇒ **H1
   also fails.** The plan's S3 says "if H2's rename unavoidably trips H1 too, the step records
   it and the mutation is adjusted" — but it does not establish that an adjustment exists that
   trips H2 *without* H1, given that the grid label is part of both the H2 label-set *and* the
   H1 schema key. This must be resolved deterministically before the step is committable, not
   "recorded and adjusted" at implementation time.
2. **G2 / G3 must be confirmed unaffected.** Renaming `era5`→`chirps` on only the COG (or
   only the Zarr) would break the shared-label coincidence (G2) or leave a basin's subtrees
   with mismatched labels. The plan must state which artifact(s) are renamed and prove G2
   still passes (no shared label to enforce on that basin) and G3 still passes.

**Suggested fix:** Pin the exact H2 mutation and prove, against `check_h1`/`check_h2`, that
the chosen rename yields H2-fail-only. Given that the grid label is in *both* the H1 schema
key and the H2 label set, an H2-only mutation may require diverging the label-*set* without
diverging any individual field's recorded `grid_label` — which may be impossible with a
single artifact rename. If so, H2 has no clean one-violation negative and that must be
documented (like the Bucket-C checks), not left to ad-hoc adjustment. Replace the "record
it and adjust" hedge with a concrete, code-verified mutation or an explicit
no-clean-negative finding.

---

## MEDIUM severity

### M-1 (scope / convention) — `architecture.md` amendment in S4 risks bundling a non-test change and must follow the amendment-log convention

**Step:** MS8-S4. The G1 probe "records the observed result as a one-line `architecture.md`
amendment." This is legitimate (architecture.md is explicitly living, §8 amendments log),
and the plan correctly limits it to "the only Rust-adjacent doc MS8 may touch." Two cautions
the plan should bake in: (a) the amendment **must** be appended to the §8 Amendments table
*newest-first* with a date, per architecture.md's own convention (lines 299-310) — not
inlined into the body; (b) if the G1 probe surfaces a genuine MS6 validator bug (e.g. a
reader admits an unnamed band that `check_g1` then mis-handles), the plan's §2 rule is
"flag as a finding, not patch inside MS8" — good, but S4's acceptance should state that a
G1-probe-induced `check_g1` change would be **out of MS8 scope** and halt the step rather
than silently editing `validate.rs` rule logic. As written S4 only edits the `validate.rs`
*test module*, which is in-scope; make the no-rule-edit guard explicit in S4's acceptance.

### M-2 (spec-drift / vague-acceptance) — S4's "G3 missing georef" lumps Zarr and COG into one Bucket-A claim, but the COG and Zarr failure paths differ

**Step:** MS8-S4. The plan's §0.1 Bucket-A row for G3 says "strip CF `grid_mapping`/CRS from
a Zarr (or georef tags from a COG) → `Err(Discovery(MissingGridGeoref))`." The Zarr path is
verified (zarr_reader resolves `grid_mapping` exclusively by following a data var's attribute;
its absence → `MissingGridGeoref`/`MissingGriddedCoordinate`). The COG path is **not** the
same error: cog_reader.rs surfaces georef via standard GeoTIFF tags and band metadata via tag
42112, with its own `CogRead`/`MissingGridGeoref` semantics (MED-4 three-outcome protocol).
The plan should pick **one** concrete G3 mutation (recommend the Zarr `grid_mapping`-strip,
which is code-verified to yield `MissingGridGeoref`) and assert the exact wrapped
`CoreError` variant, rather than the "(or … COG)" disjunction that leaves the asserted error
ambiguous. An ambiguous Bucket-A assertion is a vague acceptance criterion.

### M-3 (vague-acceptance) — S4 must confirm the T1 "non-monotonic time" fixture survives discovery (Bucket B), not fails it (Bucket A)

**Step:** MS8-S4. S4 makes T1 a Bucket-B `ran:fail`. That is correct *only if* a
non-monotonic / nullable / mis-named `time` column still **reads** through
`read_scalar_dynamic` (discovery surfaces a `TimeColumn` with the bad property) rather than
erroring. discovery.rs:235 shows a *missing* `time` column → `MissingScalarColumn` (Bucket
A), but a *present-but-unsorted* column should surface as a `TimeColumn{is_sorted: false}`
that `check_t1` then fails (Bucket B). The plan's chosen T1 mutation ("reorder one
`scalar_dynamic` `time` column so it is not ascending") is the right Bucket-B lever — but
S4's acceptance should explicitly assert the fixture **discovers successfully** and then
`check_t1` `ran:fail`s, distinguishing it from the mis-typed/nullable variants which may
land in a different bucket (a `Dtype` the reader rejects → `UnknownDtype` Bucket A). Pin
exactly one T1 leg (the sort leg) and one bucket. (Also note: the generator's existing
`assert_time_column_and_statistics` self-asserts sortedness on the *valid* baseline; the T1
mutation must run *after* the baseline self-assertions and the T1 fixture must be excluded
from that scalar self-assertion or it will abort generation — S4 should call this out.)

### M-4 (missing-coverage) — the M4 fixture covers only one of three M4 legs; the plan asserts coverage of "M4" broadly

**Step:** MS8-S2. S2 ships one M4 fixture (`manifest-bad-created-at`) and argues "empty-crs /
empty-cadence share the boundary path, documented." That is defensible (all three are
`Manifest::from_json` boundary rejects, validate.rs entry gate), and the in-memory negatives
already exist (`m4_in_memory_negative_empty_crs_and_bad_created_at_rejected`). But the plan's
coverage table claims "M4" is covered by a single fixture without an explicit assertion that
the empty-crs and empty-cadence legs are *also* regression-pinned (even if only as the
existing in-memory `Manifest::from_json` tests, not new fixtures). Make S2 state that the
empty-crs/empty-cadence legs are covered by the **retained MS1/MS6 in-memory boundary tests**
(cite them), so "M4 covered" is not an unverified blanket claim.

---

## LOW severity

### L-1 (vague-acceptance) — S6 is largely a re-affirmation; its incremental deliverable should be stated honestly

**Step:** MS8-S6. Both goldens, their snapshot/schema tests, and the ordinariness assertions
in **both** verbs already exist from MS5/MS6 (describe.rs and validate.rs test modules). S6
itself says "Expected: the valid baseline is byte-unchanged, so the goldens are unchanged."
That is correct and good (the baseline must not change), but it means S6's *new* content is
only (a) the table-driven matrix test and (b) the finalized README check-id→fixture table —
the golden/ordinariness work is re-assertion. The acceptance criteria should not imply S6
"extends" the goldens (it does not, by design); reword to "re-affirms the unchanged goldens
+ adds the consolidated matrix + finalizes the README table" so the deliverable is not
overclaimed. (Not blocking — just precision.)

### L-2 (convention) — S5's "skip-demonstration" fixtures live under `conformance/invalid/` but are conformant; naming must prevent the README table mis-implying non-conformance

**Step:** MS8-S5. The plan already flags this (a `skip-` prefix / clear labeling) and the
README must not list L3/M6/T2 demo fixtures in the "check-id → invalid-fixture" negative
table as if they pin `conformant:false`. Reinforce: the demo fixtures are **conformant**
trees, so placing them under `conformance/invalid/` is itself misleading — consider
`conformance/skip-demo/` (or a clearly separated README sub-table) so a future agent does
not read them as negatives. The plan's current "lives under `conformance/`, README documents
them as skip-demonstration" is acceptable but the directory choice should be pinned in S5's
changes, not left open ("e.g.").

### L-3 (vague-acceptance) — "advances spec checks" lines are informal; tie acceptance to concrete test names

The acceptance bullets say e.g. "Spec checks advanced (negative coverage): M3, M4, I1, I2,
I3." Per the rubric, acceptance should reference concrete, runnable checks. Each fixture step
should name the **exact new test function** it adds (e.g. `i2_folder_mismatch_pins_exactly_i2`)
and that `cargo test -p hdx-core <name>` passes, so the acceptance is mechanically checkable
rather than a prose claim. The matrix test in S6 partly addresses this, but S2–S5 should each
cite their per-fixture test names.

---

## Confirmed strengths (no action needed)

- **Bucket classification is code-true.** The A/B/C split was verified against
  `Manifest::from_json` (M2/M3/M4 entry gate), `read_outlines`'s hard `require_column`
  (I1-outlines, Geo1-missing-column → Bucket A), the zarr reader's `grid_mapping`/coordinate
  requirements (G3 → Bucket A), and the in-memory rule fns (L1/L2/I2/I3/H1/H2/T1/G2/M5 →
  Bucket B). The plan did **not** assume — it read.
- **M6 / T2 / L3 handled exactly per MED-1 / the code.** No false `conformant:false` claim
  for any R3-skip; M6 explicitly does not resurrect the dropped cross-basin rule.
- **Scope is clean.** No regrid/clip/reduce, no new `check_*`, no `build_report` change, no
  manifest field, no new domain field, no `crates/python`. The companion-mask /
  `{source}_{variable}` ordinariness is re-asserted (not newly handled) in both verbs.
- **Ordering is buildable.** S1 (framework, byte-preserving) → S2–S5 (fixtures by family) →
  S6 (matrix + README). Each step is a coherent unit; no step depends on a later one.
- **I1's leg choice is sound.** Choosing the outlines leg (Bucket A `MissingGeometryColumn`)
  for the I1 fixture is valid; the scalar-side I1 legs (Bucket B via `has_basin_id: false`)
  are reachable but the outlines leg is the cleaner one-mutation negative.

---

## Required before approval

1. **Fix H-1:** re-scope the L2 fixture so deleting/omitting an artifact does not zero a
   basin's field schema (which trips H1); prove exactly one `ran:fail`. (HIGH)
2. **Fix H-2:** pin a concrete H2 mutation proven to trip H2 *only* (not H1/L2/G2), or
   document that H2 has no clean one-violation negative given the grid label is in both the
   H1 schema key and the H2 label set. (HIGH)
3. Address M-1…M-4: explicit no-rule-edit guard for the G1 probe; one concrete code-verified
   G3 mutation + exact error variant; pin the T1 sort-leg bucket and the generator
   self-assertion exclusion; cite the retained in-memory tests for the M4 empty-crs/cadence
   legs. (MEDIUM)
