# Adversarial critique — MS6 step plan (`validate`: the §14 MUST checklist)

**Verdict: APPROVED** (severity: low). Zero high/critical issues; full coverage;
correct ordering; each step independently green and in-scope; conventions honored;
all three STEP-2 folds (MED-1, MED-2, entry-discipline/honesty) genuinely
incorporated. The remaining issues are low-severity precision/wording nits that do
not block the milestone and can be absorbed during S1/S2 implementation.

The critique was checked against the **committed code** (not just the plan's prose):
`describe.rs` entry order, the reserved `CoreError` variants, every accessor in the
"Ground truth" table, the `TimeExtent`/`TimeColumn` shapes (the MED-1 honesty), the
DTO discipline, the `era5` shared-label fixture, and the absence of any existing
`validate` surface.

---

## 1. Folds verified (the load-bearing checks)

### MED-1 (M6 cadence rule) — FAITHFULLY FOLDED ✅ (the most important fold)

The plan implements M6 as **exactly** the surviving rule and drops the
spec-unsupported cross-basin clause:

- **(a) cadence non-empty** (references M4, does not re-own it) + **(b) per-basin
  INTERNAL regularity** (uniform interior spacing) — steps.md §"The M6 cadence rule"
  (85–118), `check_m6` (441–456), scope guard (208–211), coverage map (659).
- Cross-basin "same step" equality is **DROPPED as a hard failure**; "if reported at
  all, the **FIRST R3 skip-with-reason, never a hard fail**" (98, 211, 456).
- **No cadence-word interpretation** ("never asserts `"daily"` == 1-day spacing"),
  documented limit "REGULARITY, not the cadence word" stated verbatim for the doc
  comment (99–104, 455–456).
- **Ragged per-basin extents (§6.1) never fail M6** (106, 210, 517).
- **MS8 reconciliation present**: the MS8 negative must be "an irregular per-basin
  axis fails M6; a merely-different-cross-basin-step dataset does **not**" (106), and
  the test plan pins it (517).

**This fold is technically correct and honestly grounded**, verified against the
real type: `crates/core/src/scalar_reader.rs` `TimeExtent` carries exactly
`{ start: Timestamp, end: Timestamp, source: TimeExtentSource }` — **two points, no
interior, no step**. A two-point extent genuinely cannot prove a constant interior
step, so M6 rule (b) being an honest **R3 SKIP-with-reason** in v0.1 (113–118,
447–451) is the only defensible non-vacuous, non-semantic outcome. The dataset stays
`conformant:true` because a SKIP is not a FAIL (114–116). This is exactly what the
fold demanded.

### MED-2 (testability / honesty) — FOLDED ✅

- Exit criterion **reworded** from "enforces the full list" to "**IMPLEMENTS** the
  full §14 list; positive paths on the valid fixture; unit-level negatives for
  in-memory-falsifiable checks; on-disk matrix in MS8" (13–20, 536–539).
- **Mandatory in-memory negative unit test per in-memory-falsifiable check** —
  H1, H2, I3, M3, M4, T1, G1 all carry a MANDATORY negative in S1 (327–348, 365–367);
  M5/I2 in-memory legs added in S2 (518–523).
- **On-disk matrix explicitly deferred to MS8** with the exact reserved list
  (I2 folder, T2 axis, G2 misalign, G3 georef, L1/L2/L3, Geo1, M5 crs-mismatch)
  (16–17, 524–528, 673).
- **conformant:true on the valid fixture incl. G2 positive path** on the shared
  aligned `era5` label (18–20, 465–471, 495–497) and **conformant:false on BOTH**
  invalids — M2 wrong-version, L1 missing-rollup (20, 498–512). Fixture confirmed on
  disk: `basin=0001/gridded_static/era5.tif` + `gridded_dynamic/era5.zarr` share the
  `era5` label.

### Entry discipline + honesty (§0 / §14 note) — FOLDED ✅

- `validate` performs the **§0 hard version cut + manifest boundary-parse FIRST**,
  mirroring `describe` (291–299, 322–326). Verified against `describe.rs` stages 1→4:
  `read manifest → Manifest::from_json (hard cut via early ?) → discover → assemble`.
  The plan's entry gate is a faithful mirror, and the structural-failure-vs-outcome
  split is recorded (64–66, 212–218, 677–685).
- **Every check records ran-vs-skipped + R3 depth class** (`MetadataDeep`/`ByteDeep`)
  + a skip reason (120–129, 254–264); the report **states which ran** (§14 note),
  pinned machine-readably in the S3 golden (615–619).
- **LOW-3 honored**: no check decodes a gridded chunk or pixel raster; validate runs
  over the discovery layer + the 1-D index reads MS3/MS4 already do (22–30, 124–129,
  200–202, 547).

---

## 2. Scope — CLEAN ✅

No `regrid`/`clip`/`reduce`; no inert-violating field (the report carries only id /
ran-skip / pass-fail / depth-class / opaque detail — 205–206, 360–361, 633); manifest
stays exactly six fields; `format_version` stays a hard cut. No `main.rs` change, no
new fixtures, no PyO3 (197–199). The discovery types are **not reshaped** — the one
permitted seam (the I1 `has_basin_id` accessor) is explicitly **additive**, with an
R3-skip fallback if it can't be added cleanly (219–224, 426–431). Verified: `validate`
/ `ValidationReport` / `conformant` do not exist in the tree today, so MS6 is net-new.

---

## 3. Coverage — COMPLETE ✅

Every §14 id (M1–M6, L1–L3, I1–I3, H1–H2, T1–T2, G1–G3, Geo1), every MS6 deliverable,
and every exit criterion is assigned in the coverage map (653–675) and traced to a
step. The report lists all 19 ids; S1 seeds cross-file ids as `skipped("not yet
wired")` and S2 flips them, so the shape is complete from S1 onward (300–306).

---

## 4. Ordering — SOUND ✅

S1 (report shape + pure in-memory rules + entry gate) → S2 (cross-file rules +
verdicts, builds on the frozen S1 contract) → S3 (wire-shape lock). No step depends on
a later step. Mirrors the proven MS5 discipline (shape → verb → contract-lock).

---

## 5. Green / committable — EACH STEP GREEN ✅

Each step is one conventional commit, leaves `cargo build` + `cargo test` + `cargo
clippy --all-targets -- -D warnings` green, and includes the mandated bump+tag
(319, 357, 371–372, 535, 551, 630, 641). S1's verb returns a well-formed partial report
(all 19 ids, cross-file ones skipped) so the tree is green between S1 and S2. No step
bundles unrelated changes. `jsonschema = "0.46"` is already a dev-dep (confirmed in
`crates/core/Cargo.toml`), so S3 adds no production dependency.

---

## 6. Conventions — HONORED ✅

Enums over booleans (`CheckStatus`, `CheckResult`, `DepthClass` — 250–252); `CheckId`
is an enum, never strings (247–250); fields private + getters (254–264); `thiserror`
named-field variants with *when*-it-fires doc comments for `ValidateError` (307–313);
**no `unwrap`/`expect`/panic** in library code, asserted (311, 369–370, 550); a MUST
violation is a recorded outcome, never an `Err` (64–66, 212–218); the inert domain
types gain **no** `serde::Serialize` (the S3 DTO owns the wire shape — 583–587, 633),
mirroring the verified `DescriptionDto` discipline. `additionalProperties:false` on the
schema (596). Conventional commit messages on all three steps.

---

## Issues (all low severity — non-blocking)

### LOW-1 — `[S2]` M5/Geo1 "outlines CRS / outlines column" legs rest on accessors that don't exist on the discovery layer (only on the reader types)

`check_m5` claims it cross-checks "the outlines CRS via MS4's `OutlinesInfo::crs()`"
(437–438) and `check_geo1`/`check_i1` lean on the outlines schema read (476–481,
424–426). Verified in code: `OutlinesInfo::crs()`, `has_delineation()`,
`partitioned_by_delineation()`, `has_basin_id()` **all exist on the reader type**
(`geoparquet_reader.rs`), but `GriddedDiscovery` consumes `OutlinesInfo` *inside*
`discover_gridded` (line 485) and **re-exposes only `delineations()`** — it surfaces
**no outlines CRS, no has_delineation, no partitioned-by-delineation, no
has_basin_id** through any `Discovery`/`GriddedDiscovery` accessor. The same gap as
the (correctly-flagged) I1 static-rollup seam, but the plan only names the seam for
I1's static leg — it does **not** name it for M5's outlines-CRS leg, Geo1's
schema/partition legs, or I1's outlines leg.

The plan's S2 escape hatch (485–487: "an additive accessor … or an honest R3 skip")
**does** cover this generically, so it is not a correctness hole. But the M5/Geo1
prose reads as if the facts are already reachable, which they are not. **Suggested
fix:** in S2, extend the seam note to enumerate the outlines facts (CRS,
has_delineation, partitioned_by_delineation, has_basin_id) as additive accessors on
`GriddedDiscovery` (mirroring the I1 `scalar_static_has_basin_id` seam), or state the
Geo1/M5-outlines/I1-outlines legs are honest R3 skips if they can't be surfaced
additively. This keeps the "additive, never reshape" discipline explicit for every
leg that needs it.

### LOW-2 — `[S2]` L2/L3 depend on layout facts that the discovery layer does not currently re-expose

`check_l2`/`check_l3` need per-basin gridded-subtree presence and stray/ragged-file
facts (409–420). Verified: `layout.rs` `LayoutModel` carries these
(`BasinDir::gridded_static()/gridded_dynamic().is_present()`, `is_ignored_entry`), but
the `LayoutModel` is consumed *inside* `discover_scalar`/`discover_gridded` and is
**not re-exposed** through any `Discovery` accessor; only `RootRollupPresence` (L1)
survives. Notably, an *absent* gridded subtree and a *present-but-empty* one are not
distinguishable through `GriddedDiscovery::per_basin()` (which only lists discovered
artifacts), and there is **no stray-file surface at all** post-walk. The plan
acknowledges this contingency (416–420: additive accessor or honest R3 skip), so it is
not a correctness hole — but L3 in particular ("no stray/ragged files") may have **no
cheap signal** to run on at all, making "`ran:pass` on the conformant fixture" (420)
optimistic. **Suggested fix:** in S2, state explicitly that L3 (and possibly L2's
subtree-presence leg) is the most likely honest R3 skip-with-reason in v0.1 unless an
additive `LayoutModel`-exposing accessor is added, so the implementer is not surprised
into reshaping the discovery contract to make L3 "ran".

### LOW-3 — `[S2]` M6 single-`CheckOutcome` status is left as an undecided either/or

`check_m6` says the M6 outcome is "**either** `Ran`/`Pass` with a detail naming the
R3-skipped regularity leg, **or** … `Skipped` with the regularity reason" and "chosen
and documented in S2" (450–454). Both are defensible under the fold, but leaving the
choice open weakens the S3 golden's determinism contract (the golden must pin one
concrete `status`/`result`/`detail` for M6). **Suggested fix:** pick the
`Ran`/`Pass`-with-detail form now (it is the more honest "M6 ran, its cheap leg (a)
passed, the byte-deep leg (b) is R3-deferred" reading and keeps M6 visibly *ran* in
the report), and have S3's golden assert that exact shape. Low severity because the
acceptance criteria already forbid `ran:fail` and require the regularity-leg reason;
this is a determinism tightening, not a correctness gap.

### LOW-4 — `[S2]` "M2 as Err vs as fail-outcome" left as an either/or that affects the golden and the one-violation claim

The wrong-version verdict is left as "returns `Err(Manifest(UnknownFormatVersion))`
**or** … a report with `M2` `ran:fail`" (498–507). The plan recommends the `Err`
form (matching `describe`, verified correct) and proves the observable M2 *outcome*
via an in-memory hand-built manifest fed through the M2 rule. This is sound, but two
small consequences should be nailed down: (i) if the §0 hard cut is an `Err`, then on
the wrong-version fixture there is **no `ValidationReport` at all**, so the S3 smoke
test "`validate_json` over each MS2 invalid produces valid JSON … `conformant:false`"
(624–627) can only hold for the **missing-root-rollup** invalid, not the wrong-version
one — the wrong-version path returns `Err` and emits no report JSON. (ii) The
"M2 entry-gate" pin is then a typed-error assertion, not a report outcome. **Suggested
fix:** make S3's "both invalids' reports serialize" test explicit that the
wrong-version invalid is asserted via `Err`/no-report (consistent with `describe`),
and only `missing-root-rollup` is asserted as a serialized `conformant:false` report.
Low severity — the recommended behavior is correct and consistent with `describe`; only
the test wording risks asserting a report that won't exist.

### LOW-5 — `[plan]` milestones.md MS6 prose still contains the pre-MED-1 cross-basin clause (drift between docs, not within the step plan)

`milestones.md` MS6 still says M6 = "uniformly spaced … **and consistent across
basins (same step)**" (599–606) and "axis regularity + cross-basin consistency"
(672–673), and the coverage table line 901 lists the M6 negative as "irregular axis".
The **steps.md correctly overrides this** (it explicitly removes the clause, 96–98,
and reconciles MS8, 106). So the step plan being reviewed is correct; this is a stale
milestone-prose vs step-plan drift. **Suggested fix:** the step plan is the operative
artifact and is right; recommend (out of MS6's edit scope) updating milestones.md MS6
prose + the coverage table to match the surviving MED-1 rule so a future reader of
milestones.md is not misled. Flagged for completeness; not a defect in steps.md.

---

## Bottom line

The MED-1, MED-2, and entry-discipline/honesty folds are all genuinely and
faithfully incorporated — MED-1 in particular is grounded in the real two-point
`TimeExtent`, so its R3-skip honesty is not hand-waving. Scope, coverage, ordering,
green-per-step, and conventions are all clean. The five issues are low-severity
precision tightenings (naming the additive-accessor seams for M5-outlines/Geo1/I1 and
L2/L3, locking M6's single-outcome form, and reconciling the M2-Err vs report-outcome
test wording) plus one cross-document drift in milestones.md prose that the step plan
already overrides. None block approval.
