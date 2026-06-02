# MS4 Step Plan — Adversarial Critique (CRITIC, iteration 4)

**Verdict: APPROVED.** Severity of highest issue: **low**.

The MS4 step plan (`planning/MS4/steps.md`) is byte-true against the committed MS2
fixture, fully covers every MS4 deliverable / exit criterion / spec ref in
`planning/milestones.md`, sequences five strictly dependency-ordered, independently
green steps, honors every repo convention, stays inside the MS4 boundary (no
`regrid`/`clip`/`reduce`, no transform/role/semantic/provenance field, manifest
floor untouched, no chunk/pixel decode), and genuinely incorporates all three folded
STEP-2 issues (MED-4, MED-5, LOW-3). Only minor (low-severity) observations follow;
none block approval.

**The prior HIGH defect is fixed.** Iteration 3 was NOT APPROVED because the Zarr
reader recorded cell-CENTER origin while the COG recorded cell-EDGE origin, so S5's
"the two extents coincide" assertion would have manufactured a false G2 mismatch on
two genuinely-aligned artifacts. This plan introduces the single `GridExtent`
cell-edge convention with a documented Zarr center→edge half-pixel rule
(`west = lon[0] − x_res/2`, `north = lat[0] + y_res/2`), recorded as an architecture
amendment and pinned in S1 unit tests. Both readers now emit `west=10.0 / north=50.0`,
so the coincidence assertion is byte-true. Defect resolved.

---

## Ground-truth verification (decoded at critique time)

Every number the plan pins was re-decoded from `conformance/valid/minimal/` and
matches the plan's "Ground truth" table:

| Claim in plan | Re-decoded value | Verdict |
|---|---|---|
| Zarr `lat[0]=49.875 … 48.125`, n=8, centers | `[49.875 … 48.125]`, n=8, res −0.25 | ✓ |
| Zarr `lon[0]=10.125 … 11.375`, n=6, res 0.25 | `[10.125 … 11.375]`, n=6, res 0.25 | ✓ |
| Zarr center→edge: west 10.0, north 50.0 | `lon0−res/2 = 10.0`; `lat0+res/2 = 50.0` | ✓ |
| COG transform `(0.25,0,10.0,0,−0.25,50.0)` | identical | ✓ |
| COG bounds 10.0/48.0/11.5/50.0, 6×8 | identical, width 6 height 8 | ✓ |
| `consolidated_metadata.kind == "inline"`, six members | confirmed in root `zarr.json`; members `crs, era5_precipitation, era5_precipitation_was_filled, lat, lon, time` | ✓ |
| `era5_precipitation` codec `sharding_indexed`; data at `c/0/0/0`; coords at `c/0` | confirmed; precip shape `[5,8,6]` = `[time,lat,lon]` | ✓ |
| `crs` array `shape: []`, **no** `dimension_names`; only via `grid_mapping:"crs"` | confirmed (data var attr `grid_mapping: "crs"`) | ✓ |
| **COG band description in tag 42112, NOT tag 270** | tag 270 **ABSENT**; tag 42112 GDAL_METADATA holds `<Item ... role="description">elevation</Item>` and `<Item name="units" sample="0">m</Item>` | ✓ (the MED-4 crux) |
| COG georef tags 33550/33922/34735/34736/34737, tiles 322–325 | all present | ✓ |
| geoparquet columns `[basin_id, delineation, geometry]`, 4 rows | confirmed | ✓ |
| geoparquet `geo` KV: `primary_column == "geometry"`; CRS PROJJSON top-level `id == {EPSG, 4326}` | confirmed (note: nested `datum_ensemble.id` is 6326; reader must read **top-level** `id`) | ✓ |
| delineations `{grit, merit}`; basin 0001 has both | confirmed (`basin_id` `['0001','0002','0003','0001']`, `delineation` `['merit','merit','merit','grit']`) | ✓ |
| manifest `crs == "EPSG:4326"` | confirmed | ✓ |

The half-pixel math is pinned in S1 unit tests (`center_to_edge(10.125, 0.25) == 10.0`;
`49.875 → 50.0`) and the converted edge extent is separately asserted in S2 alongside
the raw centers — making the prior-defect fix visible and regression-proof.

---

## Folded STEP-2 issues — verified incorporated (not cosmetic)

### MED-4 (COG band-description three-outcome decision) — INCORPORATED ✓

- S1 records, in the **architecture Amendments log**, a *named* three-outcome
  protocol with all three branches explicitly stated:
  (1) pure-Rust read works → G1 COG-side metadata-deep & **live**;
  (2) pure-Rust fails, GDAL accepted → record GDAL system-dependency cost as an
  amendment **and confirm the MS9 maturin/PyO3 wheel still builds with it**;
  (3) pure-Rust fails, GDAL rejected → G1 COG band-name verification is an **R3
  byte/format-deep SKIP-with-reason, never silently claimed**.
- The **never-silently-reintroduce-GDAL** rule is explicit: default is pure-Rust
  (no GDAL); outcome (2) is invoked only by an explicit follow-up amendment.
- The **round-trip on the fixture** is the executable decision: S3's test reads the
  band description back as exactly `elevation` from tag 42112 and asserts
  `CogBandSource == GdalMetadataTag` (outcome 1). I independently confirmed tag 42112
  carries `role="description">elevation` and tag 270 is absent, so the chosen reader
  target (tag 42112) is the tag the MS2 generator actually wrote — the round-trip is
  reachable on the real fixture.
- The **mismatch rule** is honored: if the reader cannot read what MS2 wrote, the fix
  is an **MS2 regenerate** (write the description in a tag the reader supports), never
  a reader workaround. Stated in both S1's amendment and S3.

This is the highest-uncertainty R1 dependency (whether the pure-Rust `tiff` crate
surfaces an arbitrary non-baseline ASCII tag like 42112). The plan does **not** assume
success: the recorded protocol decides which outcome holds at implementation time, and
the LOW crate-version contingency covers a `tiff`-crate API surprise as a recorded
pin-bump, not an ad-hoc red commit. Correct, honest handling.

### MED-5 (§8 consolidated-metadata gate, Rust-side confirmation) — INCORPORATED ✓

- S2 reads the store **via the §8 consolidated-metadata path** (one read of the root
  `zarr.json`'s `consolidated_metadata.metadata` map; I confirmed `kind == "inline"`,
  six members). It records the path taken as a self-documenting enum
  `ConsolidatedMetadataSource::Consolidated` vs `R3Skip(reason)` — **never a bool**.
- The **Rust-side confirmation** is an explicit test: it asserts the reader learned
  the store via the consolidated (inline) path and enumerated all six members from that
  single read; only if genuinely unreadable is the source an `R3Skip` with a stated
  reason, asserted, never silently claimed.
- The **regenerate-not-workaround** rule for a zarr-python vs Rust mismatch is stated.
- The `serde_json` root-`zarr.json` fallback is correctly classified as still the
  consolidated path (it reads the same inline `consolidated_metadata` object — I
  confirmed the consolidated metadata is plain JSON in the root object), preserving
  no-network / no-GDAL.

### LOW-3 (no-gridded-chunk / no-pixel review gate) — INCORPORATED ✓

- S1 records the no-chunk gate in the architecture amendment + a `grid.rs` doc note;
  the scope-guard statement repeats it.
- S2 has a **concrete asserting test**: delete/rename `era5_precipitation/c/0/0/0` in a
  temp copy and assert `read_zarr_grid` still returns identical metadata + extent (coord
  chunks `lat/c/0`, `lon/c/0`, `time/c/0` kept). I confirmed the data chunk lives at
  `c/0/0/0` and coord chunks at `c/0`, so this is a genuine behavioral proof realizable
  on the fixture.
- S3 asserts the COG reader exposes no pixel buffer and reads tags only.
- The `gridded_*` subtrees stay opaque leaves to the layout walk (already true on
  `LayoutModel.BasinDir`) and metadata-only to the readers. ✓

---

## Attack-surface findings

### SCOPE / spec-drift — clean

No step implements `regrid`/`clip`/`reduce` or any reduction/hydrology op. Every
introduced datum is a structural fact (name, quadrant, dtype, optional units, grid
label, extent/resolution, CRS string, delineation label, presence flag). The manifest
stays exactly the six floor fields (S1 explicitly adds nothing to `Manifest`). No type
carries transform/role/semantic/provenance.

The read-provenance enums (`ConsolidatedMetadataSource`, `CogBandSource`, `crs_source`)
record *which read path / which depth* — the R3 honest-skip bookkeeping the spec §14
note and architecture §1 mandate ("the validator MUST clearly report which checks
ran"). They are not "provenance of computation" about the *data* (no model / run /
pipeline). This mirrors the already-shipped `TimeExtentSource` enum in
`scalar_reader.rs`, an established in-bounds precedent. Not a violation.

No `describe` assembly / JSON-schema (MS5) or `validate` rule logic (MS6): MS4 only
reads facts and records the G1/G2-precondition/G3/Geo1/I1/M5 on-disk preconditions;
enforcement is deferred to MS6 throughout (stated per step). The G2 precondition at S5
is explicitly *observed*, not enforced.

### COVERAGE — complete

The coverage map (steps.md lines 633–654) assigns every MS4 deliverable, exit
criterion, and spec ref to a step. Cross-checked against milestones.md MS4 — all
present:

- R1 (Zarr/COG/geometry) amendment: S1 records, S2/S3/S4 fill in crate+pin. ✓
- Zarr reader (metadata, time/lat/lon coords, CF grid_mapping/CRS, units, extent/res):
  S2. ✓
- §8 consolidated-metadata gate + v3-sharding classification (MED-5): S1 gate + S2. ✓
- COG reader (band descriptions, georef, extent/res, units) MED-4: S3. ✓
- geoparquet reader ((basin_id, delineation, geometry) schema, delineation, basin_id,
  not-partitioned): S4. ✓
- CRS comparable for M5: S1 rule + S2/S3 (EPSG from files) + S4 (EPSG from PROJJSON
  `id`). ✓
- feeds unified discovery model alongside `ScalarDiscovery`: S5. ✓
- shared grid label observed across subtrees (G2 precondition) + discovery layer
  complete: S5. ✓
- LOW-3 review gate: S1+S2+S3. ✓
- spec G1/G2-precondition/G3/Geo1/I1/M5 foundations: assigned across S2–S5. ✓

No gap found.

### GREEN / committable — each step independently green

- **S1** adds pure `pub` types + new `pub` error variants + an architecture amendment.
  `pub` items in a library do not trigger `dead_code`; the new `CoreError` variants are
  constructed by the extended `every_core_error_variant_constructs` test (the plan
  explicitly says to extend it and bump its count — the current count is 15), so clippy
  `-D warnings` stays green. No reader depends on S1 to compile. Independently
  committable. ✓
- **S2/S3** each add one IO crate (Zarr / `tiff`), one reader module, fixture-backed
  tests; compile and test standalone against S1 types. ✓
- **S4** adds no new crate (reuses the present `parquet`/`arrow`/`serde_json` stack — I
  confirmed the geoparquet is a standard parquet, `PAR1` magic at both ends). ✓
- **S5** composes S2+S3+S4 + the README. ✓
- Each step is one conventional commit with `./scripts/bump-version.sh patch` + stage
  `Cargo.toml` + `git tag v<version>` — matches the CLAUDE.md mandate. Each acceptance
  block names `cargo build` + `cargo test` + `cargo clippy --all-targets -- -D warnings`.
- No step bundles unrelated changes; no step leaves the tree red. The LOW crate-version
  contingency converts an offline-unverifiable API surprise into a recorded pin-bump,
  not a red commit.

### ORDERING — buildable as written

`S1 → S2 → S3 → S4 → S5`. S1 precedes all readers. S2/S3/S4 depend only on S1 (S3/S4
are independent of S2 but sequenced after to group readers and exercise `GridInfo` with
a live reader first). S5 depends on S2+S3+S4 and reuses MS3's `walk_layout` /
`LayoutModel` (gridded subtree paths already on `BasinDir`; `discovery.rs`'s doc already
names `grids`/`delineations` as MS4-owned additions sitting alongside `ScalarDiscovery`,
"without reshaping"). No step depends on a later step.

### CONVENTIONS — honored

- `thiserror` named-field variants, each doc-commented with *when* (`ZarrRead`,
  `CogRead`, `GeoparquetRead`, `MissingGridGeoref`, `MissingGriddedCoordinate`,
  `MissingGeometryColumn`), inert/agnostic (artifact + opaque detail/column only).
  Matches existing `error.rs`. ✓
- No `unwrap`/`expect`/`panic` in any reader; `tracing` only, `#[instrument]` on public
  fns; no `println!`. ✓
- Enums over booleans for read states (`ConsolidatedMetadataSource`, `CogBandSource`,
  `crs_source`); booleans only for genuine presence facts (`has_basin_id`,
  `has_geometry`, `partitioned_by_delineation`), consistent with the existing
  `RootRollupPresence` booleans. ✓
- Parse-don't-validate: raw Zarr/TIFF/parquet bytes parsed into `GridInfo` / `Field` /
  `OutlinesInfo` at the boundary; the `EPSG:<code>` CRS-recording rule is a boundary
  parse into the `Crs` newtype. ✓
- Manifest extra/missing field handling untouched. ✓
- Private fields + getters on the new grid value types (matches `field.rs`/`layout.rs`
  style). ✓
- Crate README (complex-crate doc rule) updated in S5 with Mermaid module map +
  glossary. No `use super::*` implied. ✓

### ACCEPTANCE QUALITY — concrete

Every step's acceptance names build+test+clippy and specific, asserted spec/ground-truth
checks (exact edge extent 10.0/50.0/11.5/48.0, band name `elevation`, units `m`/`mm`,
CRS `EPSG:4326`, delineations `{grit, merit}`, basin ids, the consolidated-path
assertion, the no-chunk deletion test, the PROJJSON-`id` resolution). Commit messages
are conventional (`feat(core): …`). Not vague.

---

## Minor observations (low severity — do not block)

1. **`read_parquet_meta` reuse is metadata-only; the bounded column read is
   pattern-reuse, not function-reuse.** S4 says it reuses "the same bounded key-column
   pattern MS3 already uses." In fact MS3's bounded-read helpers
   (`read_basin_id_values_from_bytes`, `with_projection`, `leaf_column_index`, the
   `StringArray`/`LargeStringArray` extraction) are **private** to `scalar_reader.rs`;
   only `read_parquet_meta` is `pub(crate)`. So S4 will reimplement an *analogous*
   projected 1-D read in `geoparquet_reader.rs` rather than literally calling MS3's
   functions. This is fine (no new crate, still bounded, still no `geometry` blob read)
   but "reuse" slightly overstates it. Suggested fix: reword S4 to "applying the same
   bounded-projection pattern (optionally extracting a shared `pub(crate)` key-column
   helper from `scalar_reader`)" so the implementer isn't surprised that the helpers
   are private.

2. **PROJJSON has two `id` objects; the reader must take the top-level one.** The
   geoparquet CRS PROJJSON carries a nested `datum_ensemble.id == {EPSG, 6326}` and a
   top-level `id == {EPSG, 4326}`. S4 correctly reads `id.authority`+`id.code` from the
   column CRS object, and the fixture's top-level `id` is 4326 — but a naive recursive
   "first `id`" scan could hit the datum's 6326. Suggested fix: state in S4 that the
   EPSG id is the **top-level** `id` of the CRS object (not a nested `datum_ensemble.id`).
   The test asserts `EPSG:4326`, which would catch a wrong pick, so this is a clarity
   nudge, not a correctness gap.

3. **S3's no-pixel-decode assertion is necessarily soft.** The `tiff` crate may not
   expose a "no decode happened" probe, so S3 falls back to "the public API exposes no
   pixel buffer + doc/review gate." That is the honest ceiling given the crate API and
   is acceptable; it is weaker than the LOW-3 Zarr-side gate (the deletable-data-chunk
   behavioral test). No change required.

None of these affect correctness, scope, ordering, greenness, or folded-issue
incorporation.

---

## Conclusion

Zero high/critical issues. Full coverage. Correct ordering. Every step independently
green and in scope. Conventions honored. All three folded STEP-2 issues (MED-4,
MED-5, LOW-3) genuinely incorporated with executable, fixture-backed decision points
and honest R3 skip paths. The prior HIGH center-vs-edge defect is fixed by the single
`GridExtent` cell-edge convention, making S5's G2-coincidence assertion byte-true. The
plan is byte-true against the committed fixture.

**APPROVED.**
