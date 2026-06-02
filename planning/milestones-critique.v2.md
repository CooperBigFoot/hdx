# HDX v0.1 Milestone Plan — Adversarial Critique (iteration 2)

**Reviewer verdict:** APPROVED (no high/critical issues). Severity: **medium**.

The plan faithfully covers v0.1 scope (`validate` + `describe` in `hdx-core`, a
thin JSON CLI, PyO3 last), with correct dependency ordering, honest R2/R3/R4
handling, and a complete §14 coverage matrix. The remaining issues are
medium/low refinements — they do not block the plan, but several should be
folded in to avoid spec-drift risk during execution (chiefly the invented M6
cadence rule and the MS6 "all checks enforced" claim that rests on only two
demonstrated negative paths).

---

## Critical issues

None.

## High issues

None.

---

## Medium issues

### MED-1 — MS6 — spec-drift / over-reach — invented M6 "uniform spacing + cross-basin same-step" rule risks rejecting conformant datasets

**Milestone:** MS6 (and the MS8 M6 negative fixture).

The plan operationalizes spec-check **M6** ("`cadence` is consistent with the
realized `time` axes") as: *within each basin the `time` axis is uniformly
spaced (constant step) AND the step is consistent across basins (same step).*

- The within-basin uniform-spacing half is defensible: §6.2 mandates gaps are
  **NaN-filled, not dropped**, so a conformant basin axis is dense/regular by
  construction.
- The **cross-basin "same step"** half is the over-reach. The spec nowhere
  states two basins must share a sampling step beyond carrying the single
  dataset-wide `cadence` string (§6.4/§11). §6.1 explicitly permits **ragged**
  per-basin extents, and the spec is emphatic that HDX **parses no cadence
  semantics** (§1, §6.4). A dataset where basin A has a daily axis and basin B
  legitimately has a coarser realized axis is not obviously non-conformant under
  the spec text, yet the invented rule would fail it.

This is the only place the plan adds a structural requirement the spec does not
state. It is partly self-aware (the plan flags "vacuity vs semantic drift" and
provides the R3 skip-with-reason valve), which is why this is medium, not high.

**Suggested fix:** Drop or demote the cross-basin "same step" clause. Tie M6
strictly to what the spec text supports: (a) `cadence` is a non-empty string
(already M4), and (b) each basin's realized `time` axis is internally regular
(no dropped gaps — the §6.2 NaN-fill consequence). If cross-basin step
consistency is kept at all, justify it explicitly from "one dataset-wide
cadence" and make it the **first** candidate for the honest R3 skip, not a hard
fail. Update the MS8 M6 negative fixture's expected outcome accordingly (the
plan already notes the skip-then-conformant ambiguity in MS8 risks — resolve it
to match whatever M6 rule survives).

### MED-2 — MS6 — testability / reviewability — "all §14 checks enforced" exit claim rests on only two demonstrated negative paths

**Milestone:** MS6.

MS6's exit criteria assert "**All §14 MUST-checks satisfied (this is the
milestone that enforces the full list)**", yet only **2 of 19** checks (M2 via
wrong-version, L1 via missing-rollup) have an end-to-end `conformant:false`
demonstration in MS6; the other 17 fail-paths are deferred to MS8. The plan is
admirably honest about this split (critique H-2/H-3 lineage), but the
combination of "enforces the full list" + "no negative proof for 17 checks" is
internally in tension: a rule whose fail branch is never exercised is not
*proven* to fail closed at MS6.

The mitigation ("in-memory unit tests over the typed discovery model where a
negative case can be built without a writer, e.g. a hand-constructed `Vec<Field>`
with a dtype mismatch for H1") is stated as optional/per-check rather than
required, so it does not reliably close the gap.

**Suggested fix:** Make the in-memory negative unit test **mandatory** for every
check whose rule operates on the typed discovery model and can be falsified
without differently-shaped on-disk bytes — at minimum H1, H2, I3, M3, M4, T1,
G1 (rules over already-parsed structures). Reserve MS8 only for the genuinely
on-disk-shape-dependent negatives (I2 folder mismatch, T2 cross-artifact axis
mismatch, G2 misaligned shared grid, G3 missing georef, L2/L3 layout, Geo1).
Then reword the MS6 exit criterion from "enforces the full list" to "implements
the full list; positive paths proven on the valid fixture, unit-level negative
paths proven for in-memory-falsifiable checks, on-disk negative matrix in MS8."

### MED-3 — MS1 — spec-drift / testability — `deny_unknown_fields` proves "no extra field" but the plan must equally prove "exactly six" rejects a *missing* field

**Milestone:** MS1.

Spec §14 **M3** is "**exactly** the six floor fields are present." The plan's
mechanism (`serde` `deny_unknown_fields`) and its named test (7-field manifest
rejected) cover the **too-many** direction. The **too-few** direction (a
manifest missing, say, `cadence` or `producer_version`) is only implied by the
presence of a `MissingManifestField` error variant; no deliverable or
reviewable-outcome bullet asserts a 5-field manifest is rejected.

**Suggested fix:** Add an explicit MS1 deliverable + reviewable-outcome + exit
assertion: a 5-field manifest (one floor field omitted) fails the parser with
`MissingManifestField`, and likewise fails `manifest.schema.json` (the schema's
`required` array must list all six). Add the missing-field negative to the
`jsonschema` test alongside the existing 7-field case.

### MED-4 — MS4 / MS6 — R1 / honesty — pure-Rust COG band-description read is the highest-uncertainty dependency, and a GDAL fallback there silently widens the toolchain

**Milestone:** MS4 (with knock-on to MS6 G1/G3 and MS2 COG generation).

The plan's recommended pure-Rust stack uses `tiff` + GeoKey parsing for COG.
Reading **per-band descriptions** (the `G1` self-naming check: "COG band
description = field name") from a multiband GeoTIFF via the pure-Rust `tiff`
crate is genuinely uncertain — band-level `GDAL_METADATA`/description tags are
frequently the exact thing pure-Rust TIFF readers do not surface. If this read
is unreachable, the plan's stated fallback is "`gdal` for that read only," which
quietly reintroduces the GDAL **system dependency** the architecture explicitly
tried to avoid (R1) — affecting build reproducibility, CI, and the MS9 Python
wheel.

The plan acknowledges the uncertainty but does not commit to a decision point
where the GDAL-vs-skip tradeoff is made visible, nor does it state what happens
to G1 if band descriptions are pure-Rust-unreachable AND GDAL is rejected.

**Suggested fix:** In MS4, require an explicit recorded decision with three named
outcomes for COG band descriptions: (1) pure-Rust read works → G1 metadata-deep;
(2) pure-Rust fails, GDAL accepted → record the system-dependency cost as an
architecture amendment and confirm MS9 wheel still builds; (3) pure-Rust fails,
GDAL rejected → G1 band-name verification is an R3 byte/format-deep **skip with
reason**, and the COG half of G1 is reported skipped rather than silently
claimed. Make MS2's COG generator emit band descriptions in whatever tag the
chosen reader can actually read (verify the round-trip in MS2's self-assertion).

### MED-5 — MS2 / MS3 / MS4 — testability — MS2 self-assertions are Python-side; nothing guarantees the Rust reader observes the *same* engineered properties until later

**Milestone:** MS2 (with MS3/MS4 consumption).

MS2's correctness rests entirely on the **Python generator's self-assertions**
(shared grid label aligned, time stats present, consolidated metadata present,
basin_id == folder, etc.). These assert what the *writer intended*, not what a
*Rust reader can recover*. The gap between "pyarrow wrote row-group stats" and
"the `parquet` crate surfaces usable min/max for the `time` logical type" is
exactly the kind of cross-toolchain mismatch that bites (the plan even flags it
as an MS3 risk). The first Rust-side confirmation of several engineered
properties is deferred to MS3/MS4 — by which point a regenerate is costly.

**Suggested fix:** Acceptable as planned (MS3 already adds the "extent comes from
statistics, not fallback, on the MS2 fixture" assertion; MS4 adds the
consolidated-metadata gate). Strengthen it by stating in MS2 that the engineered
properties most at risk of writer/reader mismatch — `time` row-group statistics
and Zarr consolidated metadata — are explicitly the ones MS3/MS4 must confirm
**from the Rust side**, and that a failure there is a *fixture regeneration* fix,
not a reader workaround. (Largely already implied; make it a named exit linkage.)

---

## Low issues

### LOW-1 — MS3 — wording — bound and scope the parquet `time` fallback so it is not mistaken for a chunk read

MS3 describes a "bounded 1-D column-scan fallback" for time extent when stats are
absent. "Bounded" is asserted but not quantified, and for a long period of record
this is still a full single-column decode. It is still *metadata/index-tier* (one
column, not gridded chunks) per architecture §1 — state explicitly that it reads
only the `time` column (never data columns) and is therefore §1-compliant, so a
reviewer does not mistake it for a chunk read.

### LOW-2 — MS8 — fixtures — confirm the ~18 invalid trees are derived, not hand-authored

The plan already calls this out (generate programmatically from the baseline, one
surgical mutation each) and the "exactly-one-id-fails, others pass/skip"
regression assertion guards cross-contamination. No change required; keep the
mutation-from-baseline discipline as a hard rule in `conformance/README.md` so a
later contributor does not hand-edit a tree.

### LOW-3 — Plan-wide — make the "metadata + 1-D only, never gridded chunks" invariant an explicit review gate in MS4/MS6

MS5/MS6 inherit the discovery layer but neither verb's milestone re-states the
architecture-§1 backbone as an exit gate. A one-line review gate in MS4 (and
re-affirmed in MS6) — "no gridded-chunk decode anywhere in `hdx-core`" — would
make the central invariant testable rather than aspirational.

### LOW-4 — MS9 — ordering — PyO3 depends on MS8, heavier than strictly necessary (acceptable)

MS9 lists its dependency as MS8. Functionally the binding only needs MS6 (verbs)
and the schemas (MS5). Depending on MS8 is conservative and correctly keeps PyO3
strictly last, shipping the wheel against proven behavior — the safer reading of
"PyO3 mirrors them." Not a defect; documenting the rationale would pre-empt a
future "why not parallelize?" question.

---

## Cross-cutting confirmations (things the plan got right)

- **Inert/agnostic discipline upheld.** No milestone introduces a
  transform/role/semantic/provenance field on any type; MS1 explicitly forbids it
  as a risk. The manifest stays exactly six fields; `describe` reports only
  discovered facts; companion-mask and `{source}_{variable}` fields are repeatedly
  required to be treated as **ordinary** with no special casing
  (MS2/MS3/MS5/MS6/MS8).
- **format_version hard cut** is enforced **first** in MS1 (parse), MS5 (describe
  entry), MS6 (validate entry), and preserved through MS9 (PyO3 exception). M2 is
  honestly the entry gate everywhere.
- **Six-field floor not weakened by a derivable field** — MS1/MS5 both flag that
  if `describe` needs a value not in the six fields and not discoverable, the
  correct response is to flag a spec/floor bug, **never** add a manifest field
  (the §10/§11 stress-test intent).
- **Scope creep contained.** No `regrid`/`clip`/`reduce`/reduction/hydrology
  anywhere; readers are metadata-only. No full HDX writer is built — only a
  dev-only Python fixture generator in `conformance/`, explicitly not shipped in
  `hdx-core`.
- **R2 resolved at the right time.** Pulling fixture generation forward to MS2
  (before the first reader) is the correct, well-justified deviation from the §6
  hint; without it MS3/MS4 readers would be untestable. The valid fixture is
  engineered so positive paths (G2 shared-aligned grid label, §6.1
  ragged-across/§6.2 aligned-within time, basin_id folder agreement) are
  exercisable at the milestone that implements them.
- **R1 decided at the right milestones** — parquet in MS3, the hard
  Zarr/COG/geometry decision in MS4 — recorded as architecture amendments.
- **R3 honesty** — every check records ran/skipped + metadata-vs-byte
  classification; skipped checks report a reason (spec §14 note).
- **R4** — both JSON Schemas pinned (manifest in MS1, describe in MS5) and
  asserted in Rust tests via a named `jsonschema` dev-dep.
- **CLI is thin** (MS7) — no contract logic in `main.rs`, documented 0/1/2
  exit-code table with a per-code test matrix, JSON to stdout, `tracing` to
  stderr.
- **PyO3 is genuinely last** (MS9), mirrors rather than reimplements, preserves
  the hard cut, and reuses the same conformance fixtures to catch drift.
- **§14 coverage matrix is complete** — all M1–M6, L1–L3, I1–I3, H1–H2, T1–T2,
  G1–G3, Geo1 are mapped to discovery → enforcement → positive → negative,
  including homogeneity (H1/H2), intra-basin time alignment (T2),
  grid-label-implies-alignment (G2), basin_id-folder cross-check (I2), and
  multi-quadrant artifact derivation (L2 derived-from-field-set).
- **Milestone-id vs spec-check-id collision** explicitly resolved (MS1–MS9 vs
  M1–M6/etc.).

---

## Overall verdict

**APPROVED.** Zero high/critical issues. The plan is faithful to the spec,
complete against §14, correctly ordered (types → fixtures → scalar reader →
gridded/geometry reader → describe → validate → CLI → conformance suite → PyO3),
and every milestone is independently buildable + reviewable with concrete exit
criteria (build/test/clippy + specific spec MUST-check ids + bump-and-tag
commit). The medium issues (chiefly MED-1, the invented cross-basin M6 cadence
clause, and MED-2, the "all checks enforced at MS6" claim resting on two
demonstrated negatives) should be folded in during execution to keep the build
from drifting past the spec text, but none of them block the plan.
