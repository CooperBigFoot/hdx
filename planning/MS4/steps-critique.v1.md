# MS4 STEP-plan critique (adversarial review)

**Verdict: NOT APPROVED.** One HIGH issue (an unresolved closed-`Dtype` gap that
makes MS4-S2 non-green-as-written and under-specified), plus several MEDIUM/LOW
issues. The plan is otherwise strong: it is correctly scoped (metadata-only, inert,
no transform/role/semantic/provenance, no later-milestone work, no regrid/clip/
reduce), the three folded STEP-2 issues (MED-4, MED-5, LOW-3) are genuinely
incorporated with on-disk evidence, ordering is buildable, and the bump+tag /
conventional-commit discipline is honored everywhere. Fixing the HIGH issue (and
ideally the MEDIUMs) would clear it for approval.

The ground-truth claims in the plan's preamble were independently verified against
the committed MS2 fixture:

- **Zarr (MED-5):** the group `zarr.json` carries `consolidated_metadata` with
  `kind: "inline"`; every array's metadata is inlined. v3 `sharding_indexed` is on
  the two data arrays. CF georef (`crs` array with `grid_mapping_name`/`crs_wkt`/
  `spatial_ref="EPSG:4326"`; data arrays carry `grid_mapping: "crs"` + `units` +
  `dimension_names: [time,lat,lon]`; 1-D `lat`/`lon`/`time` with `standard_name` +
  `axis`). **Confirmed — MED-5 is live.**
- **COG (MED-4):** primary IFD carries tag **42112** (GDAL_METADATA, ASCII) with
  `<Item name="DESCRIPTION" sample="0" role="description">elevation</Item>` and
  `<Item name="units" sample="0">m</Item>`; tag **270** (ImageDescription) is
  **absent**; georef tags 34735 (GeoKeyDirectory), 33550 (ModelPixelScale), 33922
  (ModelTiepoint) present; SampleFormat (339)=3 (IEEE float), BitsPerSample (258)=32.
  The pure-Rust `tiff` crate exposes `Tag::Unknown(42112)` via
  `get_tag_ascii_string` / `into_string`, and `get_tag_f64_vec` / `get_tag_u16_vec`
  for the georef tags. **Confirmed — MED-4 outcome (1) is genuinely achievable in
  pure Rust; no GDAL needed.**
- **Geoparquet:** confirmed an ordinary parquet readable by the existing
  `parquet`/`arrow` path; `basin_id`/`delineation` string columns + `geometry` WKB.
  **Confirmed — no new crate needed.**

---

## HIGH

### H-1 (not-green / missing-coverage / vague-acceptance) — `int8` data field has no `Dtype` mapping; MS4-S2 cannot read the valid fixture green as written

**Step:** MS4-S2.

**The defect.** The valid fixture's `gridded_dynamic/era5.zarr` carries two **data
arrays**: `era5_precipitation` (`float32` → `Dtype::F32`, OK) and
`era5_precipitation_was_filled` (**`int8`**). MS1's closed `Dtype` enum is exactly
`{F32, F64, I32, I64, Bool, Timestamp}` (verified in `crates/core/src/field.rs`);
`parse_dtype` accepts `int8`/`i8` **nowhere** and returns
`CoreError::UnknownDtype`. S2's own deliverables say each data array becomes "one
MS1 `Field` each (`Quadrant::GriddedDynamic`, dtype via the MS1 `parse_dtype` over
the Zarr `data_type` string …)", and S2's acceptance + test plan require the reader
to discover **both** variables (including `era5_precipitation_was_filled`) as
`GriddedDynamic` `Field`s on the valid fixture. With the enum as-is, building a
`Field` for the `int8` mask **errors**, so:

- the reader cannot return the full field catalog for the valid fixture;
- the S2 acceptance test ("the two CF variables are discovered … named exactly
  `era5_precipitation` and `era5_precipitation_was_filled`") **fails**;
- S2 is **not green as written**, and (because S5 assembles all readers over the
  same fixture) S5 inherits the failure.

**Why the plan's current handling is insufficient.** S2 files this only as a
parenthetical *test-plan note*: "if `int8` is not in MS1's closed `Dtype`, this
surfaces a closed-enum gap: handle per the MS1 policy — **typed-error-first** — and
record any `Dtype` addition as an architecture amendment; do not silent-default."
This is **contradictory and under-specified**:

- "typed-error-first" taken literally means *reject* `int8` → `UnknownDtype` → S2
  RED on the valid fixture. That cannot be the intended outcome.
- The only way S2 is green is to **add an `I8` arm to MS1's closed `Dtype`** (with
  `parse_dtype` accepting `int8`/`i8`, an `as_str` arm, the `Dtype` round-trip test
  extended, and the `crates/core/README.md` glossary dtype list updated). That is a
  real, in-scope, contained change (verified: no exhaustive `match` on `Dtype`
  exists outside `field.rs`, so adding a variant will not break other modules), but
  the plan **never lists it as a planned change in S2's `Changes` section** — it is
  buried as an optional note with the wrong default.

This is exactly the MS1 risk note's "Dtype closure churn … record any addition as
an architecture amendment" case — it is *expected*, not avoidable, and must be
planned, not hand-waved.

**Suggested fix.** Make it explicit and load-bearing in **S2's `Changes`**:
"Add `Dtype::I8` to `crates/core/src/field.rs` (closed enum): a `parse_dtype` arm
accepting `int8`/`i8`, an `as_str` arm (`"i8"`), and extend the `Dtype`
round-trip / documented-string tests; update the `crates/core/README.md` glossary
dtype list (or fold that doc touch into S5's README pass, stated explicitly).
Record the `Dtype` addition in the architecture Amendments log (per the MS1
Dtype-churn policy)." Then S2's acceptance must assert the `int8` mask is read as
`Dtype::I8` (not `UnknownDtype`). Remove the contradictory "typed-error-first"
default for this known case (typed-error-first remains the rule for *genuinely
unmapped* Zarr dtypes the closed set still does not cover). Note this is a
**closed-enum extension, not a fixture problem** — the fixture is conformant
(`int8` is a legitimate mask encoding, spec §12), so this is *not* an MS2-regenerate
situation; the reader's domain dtype set was simply incomplete.

---

## MEDIUM

### M-1 (ordering / not-green) — S1 declares `i8`-dependent shapes but the enum gap is only "discovered" in S2

**Step:** MS4-S1 / MS4-S2.

S1 "freezes the gridded/geometry half's type shapes and the R1 crate choices so
S2–S4 cannot drift" and records the MED-4/MED-5/LOW-3 decisions. The `int8` mask is
a **known on-disk fact today** (the plan's own preamble does not mention it, but the
committed `zarr.json` shows `era5_precipitation_was_filled: int8`). Because S1 is
the "decisions + shared model" step and explicitly anticipates the dtype-bridge
that S2 uses, the `Dtype::I8` extension is most naturally decided **in S1** (next to
the R1-Zarr decision and the dtype-bridge documentation), not "discovered" mid-S2.
Leaving it to a S2 parenthetical is the proximate cause of H-1. At minimum, S1's
amendment entry should record the `int8` mask fact and the resulting `Dtype::I8`
decision so S2 is a mechanical application, not a fresh decision that could go the
wrong ("typed-error-first reject") way.

**Suggested fix.** Either (a) add `Dtype::I8` in S1 (it is a pure-type change,
exercised by S1's unit tests, keeps S1 green and removes the H-1 ambiguity), or
(b) keep it in S2 but have S1's amendment **name the `int8` mask and pre-commit to
adding `Dtype::I8`** so S2 cannot drift.

### M-2 (vague-acceptance) — S2's GridInfo numeric acceptance is loosely specified

**Step:** MS4-S2.

S2's test plan asserts a `GridInfo` "with the expected extent (lat over 8 cells,
lon over 6 cells, 0.25° step from the fixture) and CRS `EPSG:4326`." The fixture's
`lat` shape is 8 and `lon` is 6 (verified), and the CF arrays carry
`spatial_ref="EPSG:4326"` and a `crs_wkt`. But the acceptance does not pin **which**
CRS string is recorded verbatim — the fixture exposes *two* candidate strings
(`spatial_ref` = `"EPSG:4326"` vs the long `crs_wkt` GEOGCS WKT). Spec §7.4 /
scope-guard rule 2 say the CRS is "read and recorded verbatim … MS4 does not
interpret them." If S2 silently normalizes or picks one without stating which, that
is a hidden interpretation step (and a latent mismatch risk for MS6's M5
cross-check, where the manifest `crs` is `"EPSG:4326"`). The acceptance should
name the exact source attribute read (recommend `spatial_ref`, recorded verbatim,
no WKT parsing) so the "verbatim, no interpretation" discipline is checkable.

**Suggested fix.** In S2 acceptance: "the `Crs` is recorded **verbatim** from the
`crs` array's `spatial_ref` attribute (= `"EPSG:4326"`); the `crs_wkt` is **not**
parsed or normalized; if both are read they are recorded as-is with no
reconciliation (M5 reconciliation is MS6)." State the same for S3 (the COG GeoKey
directory yields `EPSG:4326` — confirm S3 records the same verbatim form so the
shared-label observation in S5 is comparing like with like, and so MS6's M5 sees a
consistent string across files).

### M-3 (vague-acceptance / convention) — error-variant reuse-vs-new decision is left "pick one and state it", and the lib.rs variant-count test is not flagged

**Step:** MS4-S1 (and S4).

S1 lists `GeoparquetRead { artifact, detail }` "*or* reuse `ParquetRead` and
document the reuse; pick one and state it," and similarly for the Zarr
missing-coordinate variant ("pick and document"). Leaving the choice open in the
*plan* is acceptable only if the acceptance forces a single recorded outcome;
otherwise S4's "Typed errors: `GeoparquetRead` (or documented `ParquetRead`
reuse)" can drift between steps. More concretely: `crates/core/src/lib.rs` has
`every_core_error_variant_constructs` ending in `assert_eq!(variants.len(), 15)` (a
**hardcoded count**). Any new `CoreError` variant added in S1 (`ZarrRead`,
`CogRead`, `MissingOutlinesColumn`, and possibly `GeoparquetRead`) **must** bump
that literal and add the constructor, or the test fails and S1 is RED. S1's test
plan says "extend `lib.rs`'s `every_core_error_variant_constructs`," which covers
it — but it does **not** call out the hardcoded `len()` assertion that must change
in lockstep, and a mechanical implementer could miss it.

**Suggested fix.** S1 acceptance: "decide reuse-vs-new for the geoparquet error
**in S1** and record it (recommend a distinct `GeoparquetRead` for symmetry with
`ParquetRead`/`ZarrRead`/`CogRead`); update `every_core_error_variant_constructs`
**including the `assert_eq!(variants.len(), N)` literal** to the new count, with a
constructor for each new variant." Keep `#[non_exhaustive]` and do not reshape MS3
variants (S1 already says this).

### M-4 (missing-coverage) — `crs` (int32, shape `[]`) array handling is unspecified; risk of it leaking into the field catalog or tripping the dtype bridge

**Step:** MS4-S2.

The store has a **scalar `crs` array** (`data_type: int32`, `shape: []`) that holds
the `grid_mapping` attributes — it is georef metadata, **not** a data field. S2
enumerates "data arrays (those carrying `dimension_names` of `[time,lat,lon]`)",
which correctly *excludes* `crs` (it has no `dimension_names`). Good. But S2 should
state explicitly that the `crs` array is read **for its attributes only** (it is the
`grid_mapping` target) and is **never** catalogued as a `Field` nor run through the
dtype bridge — otherwise an implementer iterating all members could mistakenly turn
`crs` into an `int32` `GriddedDynamic`/`GriddedStatic` field (it is neither, and it
has no grid label, which would also trip `Field::new`'s `MismatchedGridLabel`
invariant). The coordinate arrays (`lat`/`lon`/`time`) are likewise read for extent
only, not catalogued as fields — S2 implies this but never says it. Make both
exclusions explicit and assert in a test that the catalog is **exactly**
`{era5_precipitation, era5_precipitation_was_filled}` (no `crs`, `lat`, `lon`,
`time` fields).

**Suggested fix.** Add to S2 acceptance: "the catalog is **exactly** the two data
variables; the `crs`, `lat`, `lon`, `time` arrays are read for georef/extent only
and are **never** catalogued as fields nor passed to the dtype bridge — asserted."

---

## LOW

### L-1 (convention / ordering) — README dtype glossary line goes stale between S2 and S5

**Step:** MS4-S2 / MS4-S5.

If H-1 is fixed by adding `Dtype::I8` in S2, then `crates/core/README.md`'s glossary
line ("a closed enum — `f32, f64, i32, i64, bool, timestamp`") is **wrong** from the
end of S2 until S5 updates the README. That is not a build failure (README is not
compiled), so the tree stays green, but it is a documented-inaccuracy window that
violates the "docs reflect reality" intent. Either update the one glossary line in
S2 alongside the enum change, or have S5's README pass explicitly include the dtype
list. State which.

### L-2 (vague-acceptance) — "where feasible" on the LOW-3 no-chunk/no-pixel asserts is an escape hatch

**Step:** MS4-S2, MS4-S3, MS4-S5.

LOW-3 is folded well (scope-guard rule 1; S2 garbage-`c/` test; S3
garbled-strip-bytes test; S5 layer-level statement). But each asserting test is
hedged with "where feasible." For the two **highest-risk** readers this should be a
hard requirement, because the no-chunk/no-pixel property is the central architecture
§1 guarantee and is cheaply testable here (the Zarr hand-rolled path reads only
`zarr.json`; the `tiff` `get_tag` path provably never seeks strip/tile offsets). Make
the S2 "no `c/` chunks present → still returns full GridInfo + catalog" test and the
S3 "tags-only, pixel/strip bytes untouched" test **mandatory** (drop "where
feasible" for these two), keeping the softer phrasing only for the S5 layer-level
roll-up. Otherwise LOW-3 can be silently downgraded to a comment.

### L-3 (vague-acceptance) — S5 "the README's mermaid map compiles" is not a real check

**Step:** MS4-S5.

S5's test plan says "The README's mermaid map compiles (a doc/build sanity check)."
Markdown/Mermaid is not compiled by `cargo`, and there is no mermaid linter wired
in (MS3 didn't add one). This acceptance line is untestable as stated and should be
reworded to a concrete, reviewable obligation: "the README module map lists the
four new nodes (`zarr_reader`, `cog_reader`, `outlines_reader`, `gridded`) with
edges matching the real `use` graph in `lib.rs`'s module-map `//!` — verified by
review, not by a test." Do not claim a test that does not exist.

### L-4 (missing-coverage, minor) — S2/S3 dtype-bridge tables should enumerate exactly the closed set including the new `I8`

**Step:** MS4-S2, MS4-S3.

S2's "documented bridge mirroring `scalar_reader::arrow_dtype_str`" and S3's
"`SampleFormat`+`BitsPerSample` → `Dtype` bridge" should each pin the **exact**
closed mapping (now including `int8 → I8` for Zarr, and IEEE-float-32 → `F32` for
COG) and assert the unmapped case errors with `UnknownDtype`. S2 partially does this
("`float32`/`float64`/`int8`?/`int32`/`int64`/`bool`") — remove the `?` on `int8`
once H-1 is resolved and state the complete supported set so the test is exhaustive
rather than illustrative.

---

## What the plan gets right (for the record)

- **Scope is clean.** No regrid/clip/reduce; no transform/role/semantic/provenance/
  reduction field on any new type; the six-field `Manifest` is explicitly untouched;
  CRS strings read verbatim with the M5 cross-check correctly deferred to MS6; the
  shared grid label is *observed*, never asserted-as-aligned (G2 enforcement = MS6).
  No `describe`/`validate`/CLI/PyO3/exhaustive-invalid work bleeds in.
- **MED-4 folded correctly and with evidence.** S1 records the **three named
  outcomes**; outcome (1) is chosen with fixture proof (tag 42112 verified present,
  tag 270 verified absent, `tiff` crate verified to expose `Tag::Unknown` ASCII
  reads); GDAL explicitly **not** reintroduced; the regenerate-not-workaround rule
  and the (2)-GDAL-with-MS9-wheel-reconfirm / (3)-R3-skip fallbacks are recorded.
  S3 round-trip-verifies the exact written description (`elevation`). Genuine, not
  cosmetic.
- **MED-5 folded correctly, Rust-side.** S2 reads via the §8 inline
  `consolidated_metadata` path (verified present in the fixture), records the
  consolidated-path-used fact, asserts it in a test, and classifies the
  no-consolidated-metadata case as an R3 byte-deep skip-with-reason; sharding is
  *observed* in the codec list, not byte-verified (correct R3 line). The
  regenerate-not-reader-workaround rule is stated and matches the committed
  `conformance/README.md` hand-off note.
- **LOW-3 folded** (scope-guard rule 1 + per-reader asserts + S5 layer-level), modulo
  the "where feasible" softening flagged in L-2.
- **Ordering is buildable.** S1 (types+decisions, exercised, no new dep) → S2 (Zarr)
  → S3 (COG) → S4 (geoparquet) → S5 (unify); the MS3 seam (`LayoutModel`/`BasinDir`
  gridded subtree paths, `outlines` presence, unreshaped `ScalarDiscovery`) is real
  and used as written. Each step is one conventional commit with bump+tag.
- **Coverage matrix** maps every MS4 deliverable / exit criterion / spec ref to a
  step; the only genuine gap is the `int8` dtype handling (H-1) and the smaller
  specificity gaps above.

---

## Required before approval

1. **H-1:** make `Dtype::I8` an explicit planned change (S1 or S2) with the matching
   `parse_dtype`/`as_str`/test/README touches and an amendment entry; assert the
   `int8` mask is read as `Dtype::I8` on the valid fixture. Remove the contradictory
   "typed-error-first reject" default for this known, conformant case.
2. **M-1 / M-3:** record the `int8`→`I8` decision and the geoparquet error
   reuse-vs-new choice in S1; call out the `every_core_error_variant_constructs`
   hardcoded count.
3. **M-2 / M-4:** pin the verbatim CRS source attribute and the exact field-catalog
   exclusions (`crs`/`lat`/`lon`/`time` never catalogued) in S2/S3 acceptance.
4. **L-1…L-4:** tighten the README/dtype-doc ordering, harden the LOW-3 asserts for
   S2/S3, and reword the untestable mermaid "compiles" line.
