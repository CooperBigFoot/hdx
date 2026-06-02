# HDX v0.1 Milestone Plan — Adversarial Critique (iteration 1)

**Reviewer verdict: NOT APPROVED.**
Highest severity present: **high**.

The plan is strong on spec fidelity at the level of invariants (inert/agnostic
discipline, six-field floor, hard version cut, parse-don't-validate, per-field
quadrant, metadata-not-chunks). Ordering is broadly correct (types → fixtures →
readers → describe → validate → CLI → suite → PyO3) and PyO3 is genuinely last.
However there are real, blocking problems around **testability of specific MUST
checks before their conformance fixtures exist (G2, H1/H2, T2)**, a
**self-contradiction in M2's stated coverage vs. the M2 fixture's actual
content**, and an **ordering/dependency mismatch between M5/M6 and M7/M8** that
makes two milestones non-independently-reviewable as written. These must be
fixed before the plan can be approved.

---

## CRITICAL issues

(none)

---

## HIGH issues

### H-1 — M6: G2 positive-path enforcement has no fixture until M8 (testability hole)
- **Milestone:** M6 (also touches M2, M4)
- **Category:** testability / fixtures / ordering
- **Problem:** M6's exit criteria require **all** §14 checks implemented and the
  M2 fixture to validate as `conformant: true`, including **G2** (shared grid
  label across the `gridded_static`/`gridded_dynamic` subtrees ⇒ cell-for-cell
  alignment). But the M2 fixture deliverable (M2) only specifies
  `gridded_static/<label>.tif` and `gridded_dynamic/<label>.zarr` — it never
  states the two labels are the *same* label, which is the entire precondition
  for G2 to *fire* in the positive direction. The "shared-grid-label aligned"
  valid dataset is introduced only in **M8**. So when M6 implements G2 and runs
  it against M2, there is likely **no positive case exercising the alignment
  comparison** — G2 either no-ops (no shared label present) or is asserted
  against a fixture that was never designed to have a shared label. A check that
  never fires against the only available fixture is not actually tested at M6.
- **Suggested fix:** Make M2's minimal valid fixture use **one shared grid
  label** across `gridded_static/<label>.tif` and `gridded_dynamic/<label>.zarr`
  (cell-for-cell aligned), so G2's positive path is exercisable the moment it is
  implemented in M6. Add to M2's exit criteria: "the COG and Zarr in each basin
  share a grid label and are cell-for-cell aligned (seeds G2 positive path)."
  Alternatively, explicitly move the G2-positive fixture earlier and state in M6
  that G2 enforcement depends on it.

### H-2 — M6: H1/H2 and T2 positive-path require ≥2 basins with *identical* schema, but M6 cannot fail-test them until M8
- **Milestone:** M6
- **Category:** testability
- **Problem:** M6 implements H1 (identical field schema across basins), H2
  (identical grid-label set across basins) and T2 (intra-basin axis alignment
  across `scalar_dynamic` + every `gridded_dynamic`). M2 provides ≥2 basins, so
  the *positive* path is testable. But M6's deliverables claim "targeted unit
  tests per check id using small inline inputs" — for H1/H2/T2/I2/I3/G2 the
  negative path needs a *differently-shaped* dataset (extra field in one basin,
  differing grid-label set, mismatched axis, folder/id disagreement, duplicate
  id). Constructing these as "small inline inputs" in Rust is exactly the thing
  the plan elsewhere (M2 rationale, R2) says is **infeasible without a writer**.
  The plan cannot have it both ways: either the readers can be unit-tested with
  synthesized in-memory inputs (in which case R2/M2's whole justification
  weakens), or they cannot (in which case M6's per-check negative tests must wait
  for M8 invalid fixtures). As written, M6's exit criterion "targeted unit tests
  per check id" is not honestly achievable for the cross-basin/byte checks.
- **Suggested fix:** Split M6's testing honestly: state that **positive** checks
  are exercised against the M2 valid fixture at M6, and that **negative
  (fail-path) per-check tests are deferred to M8** where the one-violation-per-
  invalid fixtures live (M8 already owns this). Remove or qualify the "targeted
  unit tests per check id" claim in M6 so it does not assert coverage that only
  M8 delivers. Make the M6↔M8 split of positive-vs-negative testing explicit so
  M6 is honestly reviewable on its own.

### H-3 — M5/M6 depend on M4, but M7/M8/M9 dependency edges make M5 and M6 not independently shippable as "reviewable outcomes"
- **Milestone:** M5, M6 (dependency-graph correctness)
- **Category:** ordering / decomposition
- **Problem:** The Mermaid graph and prose say the critical path is
  `M1→M2→M3→M4→M5→M6→{M7,M8,M9}`, and that M5 (`describe`) lands **before** M6
  (`validate`). That ordering is defensible. **But** M5's only reviewable
  artifact is a golden-`describe` test over the M2 fixture, and M6's only
  reviewable artifact is `validate` returning `conformant: true` over the *same
  single* M2 fixture. Neither milestone can demonstrate a **fail** outcome
  (`describe` over a malformed dataset, `validate` returning `conformant:false`)
  because the invalid fixtures do not exist until M8. A `validate` implementation
  that has *never been observed to return false* in its own milestone is not
  meaningfully reviewable — the most important behavior of a validator (failing
  closed) is untested at the milestone that builds it. This is the same root
  cause as H-1/H-2 but stated at the decomposition level: M6 is not
  independently reviewable for its core promise.
- **Suggested fix:** Pull a **minimal invalid fixture or two** (e.g. wrong
  `format_version`, missing root rollup) forward into M2 (or a small dedicated
  step at the head of M6), so M6 can demonstrate at least one `conformant:false`
  verdict within its own milestone. Keep the *exhaustive* one-per-check invalid
  family in M8. State explicitly in M6's reviewable-outcome that a `false`
  verdict is demonstrated, not only a `true` one.

---

## MEDIUM issues

### M-1 — M2 exit criteria over-claim structural coverage of checks the fixture does not actually exercise
- **Milestone:** M2
- **Category:** spec-drift (coverage accounting) / testability
- **Problem:** M2's exit criteria say the fixture "is hand-verified to satisfy
  the relevant layout/identity/time/grid MUST checks … **L1, L2, I1, T1, G1, G3,
  Geo1** are structurally present." Two problems: (a) **G2** is conspicuously
  absent from this list even though M2 produces both gridded subtrees — and per
  H-1 it should be present-and-shared; (b) listing **I1/T1/G1/G3/Geo1** as
  "structurally present" in a hand-verified fixture is fine, but the milestone
  presents these as a coverage claim without the programmatic check existing yet,
  which risks being read as "covered." The coverage ledger should clearly mark
  these as *seeded, not enforced*.
- **Suggested fix:** Add G2 to M2's structural list (tied to H-1's shared-label
  fix). Relabel the list as "structurally seeds (enforced later in M3–M6):" to
  avoid implying enforcement. Ensure the plan's overall coverage matrix
  distinguishes *seeded-by-fixture* from *enforced-by-check*.

### M-2 — M2 ragged-time + I2/I3 not seeded; cross-basin checks lack positive fixture diversity
- **Milestone:** M2
- **Category:** fixtures / coverage
- **Problem:** M2 says basins share an identical schema "but with ragged time
  extents between basins (§6.1)" — good for the §6.1 positive case. But the
  fixture must also positively exercise **I2** (in-file `basin_id` agrees with
  folder) and **I3** (uniqueness across the dataset) — with ≥2 basins this is
  implicitly present, but the exit criteria omit I2/I3 from the seeded list, so a
  reviewer cannot confirm the fixture's basin folders and in-file ids are
  deliberately consistent (e.g. the generator could accidentally write the same
  id in two folders, or a folder name that does not match the column). Silent
  omission risks a fixture that passes by luck rather than design.
- **Suggested fix:** Add **I2, I3** (and the §6.1 ragged-extent property
  explicitly) to M2's seeded-checks list, and add a generator self-assertion that
  folder `<id>` equals the in-file `basin_id` and that ids are unique.

### M-3 — M6 "cadence consistent with realized time axes" (M6 check) is under-specified and risks scope creep or vacuity
- **Milestone:** M6
- **Category:** spec-drift / testability
- **Problem:** Spec M6 says "`cadence` is consistent with the realized `time`
  axes." The plan restates this verbatim but never says *how* consistency is
  decided from a free-form opaque `Cadence(String)` (the type model deliberately
  parses no semantics out of cadence — §11/architecture). Without a defined
  comparison, this check is either (a) **vacuous** (always passes), which should
  be reported as `skipped` with a reason under R3, or (b) it requires HDX to
  *interpret* the cadence string ("daily" ⇒ uniform 1-day spacing), which edges
  toward semantic interpretation the spec keeps HDX out of. The plan must pick
  and state which, or it will drift.
- **Suggested fix:** In M6, explicitly define the M6-check semantics and classify
  it under R3: either implement a concrete, non-semantic consistency rule (e.g.
  the realized axis spacing is uniform and the manifest cadence string is
  non-empty / present) and document its limits, or declare M6-check **`skipped`
  with a stated reason** rather than silently passing. Note the tension that HDX
  parses no cadence semantics, and ensure the chosen rule does not turn cadence
  into an interpreted field.

### M-4 — M5 declares "no conformance verdict" but must still surface read failures; describe-over-malformed behavior is unspecified
- **Milestone:** M5
- **Category:** testability / spec-drift
- **Problem:** M5 says `describe` "reports facts only (no conformance verdict)".
  Correct per §10. But `describe` must still **fail** on a dataset it cannot read
  (unknown `format_version` — the hard cut applies to *both* verbs per §0 "before
  anything else in the dataset"; missing manifest; unreadable parquet). The plan
  never states whether `describe` applies the hard version cut and the boundary
  parse before discovery. If it does not, `describe` would violate §0's "readable
  before anything else." This is a real spec-fidelity gap, not cosmetic.
- **Suggested fix:** Add to M5: `describe` performs the `format_version` hard cut
  and manifest boundary-parse first (§0), erroring on unknown version / missing
  manifest before any discovery; only *discovery gaps* (not conformance
  violations) are reported as facts. Add a test that `describe` over a
  `format_version "0.2"` input errors with `UnknownFormatVersion`.

### M-5 — M4 CRS comparison: "defer the actual M5 cross-check rule to M6" mislabels the check id
- **Milestone:** M4 (and M6)
- **Category:** spec-drift (check-id labeling)
- **Problem:** M4's risk note says "defer the actual M5 cross-check rule to M6"
  — here "M5" means **spec check M5** (`crs` matches file CRS), but the document
  also uses "M5" as the **milestone** id (`describe`). This collision is
  genuinely confusing in a plan whose milestones are M1–M9 and whose manifest
  checks are M1–M6. A reviewer cannot tell at a glance whether "M5 cross-check"
  is milestone M5 or manifest check M5. Given the milestone numbering reuses the
  exact tokens M1–M6 that are also manifest check ids, every such reference is
  ambiguous.
- **Suggested fix:** Disambiguate throughout: write manifest checks as `[M5]` or
  "check M5" / "spec §14 M5" and milestones as "Milestone 5 (M5)". Best: rename
  milestones to a non-colliding scheme (e.g. `MS1..MS9` or `S1..S9`) so milestone
  ids never collide with the §14 manifest check ids M1–M6. This is a real
  reviewability defect, not pedantry, because M1/M2/M3/M4/M5/M6 are *both*
  milestone ids *and* manifest check ids in the same document.

### M-6 — M3 time-extent reading may need full-column scan; "row-group statistics suffice" is an unverified assumption
- **Milestone:** M3
- **Category:** testability / risk (metadata-vs-byte honesty)
- **Problem:** M3 records per-basin `[start,end]` time extent "without loading
  full series where row-group statistics suffice." Parquet min/max statistics are
  *optional* and may be absent or, for timestamp logical types, not written by
  every writer. The plan's own M2 generator is Python/pyarrow; whether it writes
  per-column timestamp statistics is not asserted. If stats are missing, the
  reader must scan, contradicting the "metadata + 1-D index reads only" backbone
  for the extent. The fallback ("bounded column scan") is mentioned but the
  metadata-vs-byte classification of *time-extent reading* is not pinned, which
  is exactly the R3 honesty concern.
- **Suggested fix:** In M2, assert the generator writes row-group statistics for
  the `time` column (§8 already mandates "row-group statistics written"). In M3,
  state that time-extent comes from parquet statistics, and classify the
  bounded-scan fallback explicitly under R3 (it reads the `time` column, a 1-D
  index read, which is allowed; but say so). Add a test that the M2 fixture's
  `time` column carries usable statistics.

### M-7 — R1 reader-crate decision is recorded at M4, but the §8 "consolidated metadata" / "v3 sharding" reads are conformance-relevant and may be unreachable in `zarrs`
- **Milestone:** M4
- **Category:** reader-crates / risk R1 / honesty
- **Problem:** Spec §8 mandates Zarr **v3 sharding** and **consolidated
  metadata**. The plan's G3 check is "CF georef present" and the plan defers
  literal sharding/overview verification to byte-deep/R3. Good. But M4's R1
  default leans on `zarrs` for "consolidated metadata" reads, and M2's own risks
  flag uncertainty about the Python `zarr` version's consolidated-metadata
  support. If the *fixture* cannot be written with consolidated metadata, or
  `zarrs` cannot read it, the plan has a latent contradiction between "one GET to
  learn the store" (§8) and the chosen toolchain. The plan acknowledges this as a
  risk but does not gate M4 on a resolution, so M4 could be declared "done" with
  a reader that silently cannot do consolidated-metadata reads.
- **Suggested fix:** Make M4's exit criteria explicit that the chosen Zarr reader
  reads the M2 fixture's metadata via the path §8 mandates (consolidated
  metadata) **or** that the consolidated-metadata / sharding verification is
  classified as a byte-deep R3 `skipped` check with a reason. Tie the M2 fixture
  self-check (already listed as a risk mitigation) to M4's gate so the two cannot
  diverge.

---

## LOW issues

### L-1 — M2's dependency on M1 is conceptual, not a build dependency; state this so M2 is not blocked spuriously
- **Milestone:** M2
- **Category:** ordering
- **Problem:** M2's fixture generator is **Python** and links nothing from
  `hdx-core`. Its dependency on M1 is "the fixtures must match the manifest shape
  M1 defines" — a *spec-shape* coupling, not a code/build dependency. As written
  ("Dependencies: M1") a reviewer may think M2 needs M1's Rust types compiled.
  The real constraint is only that the manifest JSON shape is frozen (it is, by
  spec §11). Minor, but it muddies the dependency graph.
- **Suggested fix:** Reword M2's dependency as "M1 (shape only — the frozen
  manifest/field model; no `hdx-core` linkage)." This also makes clear M2 could
  in principle proceed in parallel with M1's Rust work once the manifest shape is
  agreed.

### L-2 — Exit-code scheme stated in M7 risk note but not in M7 deliverables/exit criteria
- **Milestone:** M7
- **Category:** decomposition (concrete exit criteria)
- **Problem:** M7's risk mitigation proposes "0 ok, 1 non-conformant, 2 usage/IO
  error" but the deliverables/exit criteria only say "non-zero iff conformant ==
  false" and "non-zero on IO/parse errors," collapsing 1 and 2. For an
  LLM-drivable CLI (§10), distinct exit codes for non-conformant vs error are
  load-bearing for scripting. Leaving it in a risk note rather than the contract
  makes M7 under-specified.
- **Suggested fix:** Promote the exit-code table (0 conformant/success, 1
  non-conformant, 2 usage/IO error) into M7's deliverables and exit criteria, and
  add a CLI test asserting each code.

### L-3 — "schemas/" pinned twice (manifest in M1, describe in M5) but no JSON-Schema validation tooling/dep is named
- **Milestone:** M1, M5
- **Category:** testability
- **Problem:** M1 and M5 commit JSON Schemas to `schemas/` and the reviewable
  outcomes say a reviewer "can validate a sample against the schema," but neither
  milestone names *how* (a Rust `jsonschema` dev-dependency? an external tool?).
  Without a pinned validation mechanism, "validates against the schema" is a
  manual hope, not a test.
- **Suggested fix:** Name the validation mechanism (e.g. a `jsonschema`
  dev-dependency used in a test that asserts the §11 example validates against
  `manifest.schema.json`, and that the M2 golden `describe` validates against
  `describe.schema.json`). Add it to M1/M5 deliverables.

### L-4 — Companion-mask / naming-pattern neutrality (§2) is never exercised as a non-check
- **Milestone:** M2, M6
- **Category:** spec-drift (negative coverage)
- **Problem:** Spec §2 is emphatic that companion masks (`{field}_was_filled`)
  and naming patterns (`{source}_{variable}`) get **no magic** and a validator
  **MUST NOT** depend on them. The plan never includes a fixture field with a
  `_was_filled` suffix or a `{source}_{variable}` name to *prove* the validator
  treats them as ordinary fields (no special-casing). A latent regression (some
  future contributor adding suffix logic) would go uncaught.
- **Suggested fix:** Add to M2's field schema at least one companion-mask-named
  field and one `{source}_{variable}`-named field, and add an M6/M8 assertion
  that they are catalogued as ordinary fields with no special handling.

### L-5 — `Dtype` enum is left as a `…` open set in M1; the boundary parse needs a closed mapping or an explicit "opaque/unknown" arm
- **Milestone:** M1
- **Category:** parse-don't-validate / testability
- **Problem:** M1 lists `Dtype` among deliverables but (mirroring the
  architecture sketch `Dtype { f32,f64,… }`) leaves the set open. Parse-don't-
  validate requires the boundary parse to map every parquet/Zarr/COG physical
  dtype it may encounter to a `Dtype`; an un-mapped dtype must have a defined
  outcome (error vs an `Other` arm). Unspecified, this is a latent panic/`unwrap`
  risk in library code (which CLAUDE.md forbids).
- **Suggested fix:** In M1, require `Dtype` to either be a closed enum with a
  documented mapping and a fallible parse that *errors* on unknown physical
  types, or include an explicit `Other(String)`/`Unsupported` arm. State the
  no-panic guarantee for unknown dtypes.

---

## Coverage ledger (spec §14 MUST → milestone)

| Check | Readable/seeded | Enforced | Negative fixture | Notes |
|---|---|---|---|---|
| M1 (manifest exists, fv read first) | M1 (parse) / M3 (on-disk) | M6 | M8 | ok |
| M2 (fv hard cut) | M1 | M6 | M8 | ok |
| M3 (exactly six fields) | M1 | M6 | M8 | ok |
| M4 (RFC3339; non-empty crs/cadence) | M1 | M6 | M8 | ok |
| M5 (crs matches files) | M4 (read) | M6 | M8 | label collision (M-5) |
| M6 (cadence consistent) | M4 (read) | M6 | M8 | under-specified (M-3) |
| L1 (root rollups) | M2/M3 | M6 | M8 | ok |
| L2 (basin shape + cond. gridded) | M2/M3 | M6 | M8 | ok |
| L3 (no stray/ragged) | M3 | M6 | M8 | ok |
| I1 (basin_id present) | M2/M3 | M6 | M8 | ok |
| I2 (in-file id = folder) | M3 | M6 | M8 | M2 seeding omitted (M-2) |
| I3 (unique) | M3 | M6 | M8 | M2 seeding omitted (M-2) |
| H1 (identical schema) | M3/M4 | M6 | M8 | positive ok; neg only at M8 (H-2) |
| H2 (identical grid-label set) | M4 | M6 | M8 | positive ok; neg only at M8 (H-2) |
| T1 (time named/typed/sorted/non-null) | M3 | M6 | M8 | ok |
| T2 (intra-basin axis aligned) | M3+M4 | M6 | M8 | positive ok; neg only at M8 |
| G1 (one-grid/self-naming) | M4 | M6 | M8 | ok |
| G2 (shared-label ⇒ alignment) | **gap** | M6 | M8 | **no positive fixture until M8 (H-1)** |
| G3 (CF/GeoTIFF georef) | M4 | M6 | M8 | consolidated-metadata caveat (M-7) |
| Geo1 (outlines schema/label/partition) | M4 | M6 | M8 | ok |

Every check is assigned, but **G2 lacks a positive fixture at the milestone that
enforces it (H-1)**, and the negative (fail-path) coverage for the cross-basin /
alignment / identity checks is concentrated entirely in M8 while M6 claims
per-check tests (H-2/H-3).

---

## What the plan gets right (so it is not lost in revision)

- Inert/agnostic discipline is held everywhere; no transform/role/semantic/
  provenance leaks into any type, reader, report, or surface. Cross-cutting
  guardrails restate this and the manifest floor explicitly.
- Six-field manifest floor is correct; M1 enforces `additionalProperties:false`
  and an extra-key rejection test; no milestone adds a derivable field.
- `format_version` hard cut is read-first and version-rejected at the boundary.
- No `regrid`/`clip`/`reduce`/reduction/hydrology anywhere; explicitly excluded
  and guarded.
- Per-field quadrant + artifacts-derived-from-field-set is correct (scalar-only ⇒
  no `gridded_*`), and M8 adds a scalar-only valid fixture to prove derivation.
- Metadata-not-chunks backbone is enforced with "no chunk decode" assertions in
  M3/M4.
- Ordering is sound: discovery layer (M3+M4) is built before the verbs (M5/M6)
  that consume it; CLI (M7) is thin glue with no contract logic; PyO3 (M9) is
  genuinely last and mirrors-not-reimplements.
- No HDX writer is built; only a dev-only Python fixture generator (R2), never
  linked into `hdx-core`.
- R1/R3/R4 are each assigned to a milestone (M4/M6/M5 resp.) with recorded
  architecture amendments.

---

## Required-before-approval checklist

1. **H-1:** M2 minimal fixture must use a shared grid label (aligned) so G2's
   positive path is exercisable at M6.
2. **H-2 / H-3:** Honestly split positive (M6, against M2) vs negative (M8)
   per-check testing; ensure M6 demonstrates at least one `conformant:false`
   verdict within its own milestone (pull one or two invalid fixtures forward).
3. **M-1/M-2:** Fix M2's seeded-checks ledger (add G2, I2, I3, ragged-extent;
   relabel as seeded-not-enforced) with generator self-assertions.
4. **M-3:** Define or honestly skip the cadence-consistency (check M6) rule under
   R3 without interpreting cadence semantics.
5. **M-4:** State that `describe` applies the §0 hard cut + boundary parse before
   discovery.
6. **M-5:** Disambiguate milestone ids from §14 manifest check ids M1–M6.

Fix the HIGH items (H-1, H-2, H-3) and the spec-fidelity MEDIUMs (M-3, M-4) and
the plan should clear; the LOWs are polish.
