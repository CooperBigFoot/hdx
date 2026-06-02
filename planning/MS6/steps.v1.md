# MS6 — `validate`: the §14 MUST checklist over the discovery layer

> **Milestone scope (verbatim intent, milestones.md MS6).** Implement `validate`
> (spec §10): run the complete §14 `MUST` checklist (M1–M6, L1–L3, I1–I3, H1–H2,
> T1–T2, G1–G3, Geo1) over the **shared discovery layer** (MS3+MS4), producing a
> `ValidationReport` of per-check outcomes (ran / skipped, pass / fail, detail) and an
> overall `conformant: bool`. **Fails closed** on any violated MUST. Make the **R3**
> metadata-deep vs byte-deep depth decision explicit per check and report skipped
> checks honestly (spec §14 note). Demonstrate **both** verdicts within this milestone:
> `conformant:true` on the MS2 valid fixture and `conformant:false` on MS2's two
> invalids.
>
> **Reworded exit criterion (FOLD MED-2).** MS6 **IMPLEMENTS** the full §14 list;
> **positive paths** are proven on the MS2 valid fixture; **unit-level negative paths**
> are proven for every check whose rule is **in-memory-falsifiable** over the typed
> discovery model (at minimum H1, H2, I3, M3, M4, T1, G1); the **on-disk negative
> matrix** (I2, T2, G2, G3, L1/L2/L3, Geo1, Geo-related, M5 crs-mismatch) is completed
> in **MS8**. MS6 demonstrates `conformant:true` on the valid fixture (incl. spec-check
> G2's positive path on the shared aligned grid label) and `conformant:false` on **both**
> MS2 invalids (M2 wrong-version, L1 missing-rollup).
>
> **Hard boundaries (do not cross).** No `regrid` / `clip` / `reduce`, ever. No
> inert-violating field anywhere (no transform / role / semantic / provenance, no
> derivable manifest field; the manifest stays **exactly the six floor fields**;
> `format_version` is a **hard cut**). `validate` adds **no reader** and decodes **no
> gridded chunk / pixel raster** (LOW-3) — it runs over the discovery layer + the small
> 1-D index reads MS3/MS4 already perform. It must **not reshape** `Discovery` /
> `ScalarDiscovery` / `GriddedDiscovery` / `Field` / `GridInfo` / `TimeExtent` (those
> are MS3/MS4 contracts). `validate` interprets **no cadence word** and asserts **no
> cross-basin time-extent equality** (§6.1 permits ragged extents).

---

## Ground truth (verified against the committed code + MS2 fixture before planning)

`validate` is a **rule pass over an already-typed model**; it adds no reader. The
shapes it consumes are fixed by MS3/MS4/MS5 and were read at plan time. The single
discovery entry point is `gridded_discovery::discover(path) -> Discovery`, and the
manifest entry point is `manifest::Manifest::from_json(&str)`. The verb mirrors
`describe`'s proven §0 entry order (`describe.rs` stages 1→4).

| Source (already in `hdx-core`) | What `validate` reads from it | Which §14 check it feeds |
|---|---|---|
| `Manifest::from_json` (hard-cuts `format_version` first; six-field parse) | the entry gate; surfaces `UnknownFormatVersion` / `ExtraManifestField` / `MissingManifestField` / `InvalidTimestamp` / `EmptyCrs` / `EmptyCadence` | M1, M2, M3, M4 |
| `Manifest::crs() -> &Crs`, `Manifest::cadence() -> &Cadence` | the manifest CRS + cadence string | M5 (vs file CRS), M6 (cadence non-empty + axis regularity) |
| `Discovery::scalar().root_rollups()` (`scalar_static_present()` / `outlines_present()`) | root-rollup presence facts | L1 |
| `Discovery::scalar().basins() -> &[BasinId]` | the folder-id basin list | I3, H1, H2, T1, T2 (per basin) |
| `Discovery::scalar().per_basin() -> &[BasinScalar]` (`basin_id_folder()`, `basin_id_in_file() -> Option<&BasinId>`, `time() -> Option<&TimeColumn>`, `time_extent() -> Option<TimeExtent>`, `fields() -> &[Field]`) | per-basin id pair (I2), `time` descriptor (T1), extent (M6/T2 inputs), per-basin scalar schema (H1) | I1, I2, T1, T2, M6, H1 |
| `ScalarStaticTable::has_basin_id()` (via the static rollup read inside `discover_scalar`) | already folded into the scalar half — **note:** the scalar half does **not** currently surface `scalar_static`'s `has_basin_id` on `ScalarDiscovery`; see the S2 seam note | I1 (static rollup) |
| `Discovery::gridded().per_basin() -> &[BasinGridded]` (`static_grid_labels()`, `dynamic_grid_labels()`, `static_artifacts()` / `dynamic_artifacts()` each → `grid_info() -> &GridInfo`, `consolidated_source()`) | per-basin grid-label sets (H2), the shared-label fact + the two `GridInfo` extents (G2), the MED-5 Zarr path (R3 honesty) | H2, G2, G3 |
| `Discovery::grids() -> &[GridInfo]` (`grid_label()`, `extent()`, `resolution()`, `width()`, `height()`, `crs() -> &Crs`) | per-grid geometry + recorded CRS | M5, G3 |
| `Discovery::gridded().gridded_fields() -> &[Field]` | the gridded schema with `quadrant`/`grid_label` (`Some` iff gridded — `Field::new` invariant) | H1, G1 |
| `Discovery::fields() -> Vec<&Field>` | unified `scalar ⊕ gridded` catalog | H1, G1 |
| `TimeColumn` (`name()`, `dtype()`, `is_nullable()`, `is_sorted_ascending()`) | the four T1 facts (recorded by MS3, enforced here) | T1 |
| `TimeExtent` (`start()`/`end()` → `Timestamp::as_offset_date_time()`, `source()`) | per-basin `[start,end]` + read-tier provenance | M6, T2, R3 reporting |
| `ConsolidatedMetadataSource` (`Consolidated{members}` / `R3Skip{reason}`) | which Zarr path was taken (live vs R3 skip) — honest R3 reporting | the §8/G3 byte-deep R3 note |

**Reserved error variants already present (`error.rs`, declared in MS1, unused until
now):** `BasinIdFolderMismatch{in_file,folder}` (I2), `RaggedSchema{basin}` (H1),
`GridLabelMismatchAcrossBasins{label}` (H2), `MissingRootRollup{artifact}` (L1),
`NonMonotonicTime{artifact}` (T1). MS6 is the milestone that **wires these in**.
`validate` does **not** fail through `CoreError` for a violated MUST, though — a MUST
violation is a **recorded per-check outcome** in the `ValidationReport` (fail-closed via
`conformant:false`), not a returned `Err`. `CoreError`/the new `ValidateError` is for
**structural** failures (unreadable dir, undecodable present artifact, unreadable
manifest) and the §0 hard cut — see the entry-gate discipline below.

**Decoded facts of the MS2 valid fixture (`conformance/valid/minimal/`)** — these make
the `conformant:true` proof and spec-check G2's positive path concrete:

| Fact | Value | Feeds |
|---|---|---|
| manifest | `{format_version:"0.1", name:"hdx-conformance-valid-minimal", created_at:"2026-06-01T00:00:00Z", producer_version:"hdx-fixtures 0.1.0", crs:"EPSG:4326", cadence:"daily"}` | M1–M4, M5, M6 |
| basins | `["0001","0002","0003"]` | I3, H1, H2 |
| in-file `basin_id` == folder id, each basin; unique | yes (`0001`/`0002`/`0003`) | I1, I2, I3 |
| scalar `time` | name `time`, `timestamp`, non-nullable, sorted ascending | T1 |
| per-basin time extents | 0001 `[2000-01-01,2000-01-05]`, 0002 starts `2010-06-15`, 0003 starts `2005-03-01` (ragged §6.1), all `source==Statistics` | M6, T2 |
| gridded fields | `elevation` (GriddedStatic, label `era5`), `era5_precipitation` + `era5_precipitation_was_filled` (GriddedDynamic, label `era5`) | G1, H1 |
| grid `era5` | COG **and** Zarr both report `era5`, both extents coincide at west=10.0 north=50.0 east=11.5 south=48.0; res x=0.25 y=−0.25; 6×8; crs `EPSG:4326` | G2 (shared aligned label), G3, M5 |
| delineations | `{grit, merit}` | Geo1 (read by MS4) |
| Zarr MED-5 path | `Consolidated` with 6 members (live) | R3 honesty |
| `invalid/wrong-format-version` | identical but `format_version:"0.2"` | M2 → `conformant:false` |
| `invalid/missing-root-rollup` | identical but `outlines.geoparquet` absent at root | L1 → `conformant:false` |

**The M6 cadence rule this milestone adopts (FOLD MED-1 — the most important fold).**
Spec §1/§6.4 say HDX **parses no cadence semantics**; §6.1 explicitly permits **ragged
per-basin time extents**. Therefore spec-check **M6 is implemented as EXACTLY**:

  (a) `cadence` is a **non-empty string** (this is also M4 — M6 references it, does not
      re-own it); **AND**
  (b) **each basin's** realized `time` axis is **INTERNALLY regular** — uniformly spaced
      within that basin (the §6.2 consequence: gaps are NaN-filled, never dropped, so a
      conformant per-basin axis has a constant step).

  M6 **DROPS** the cross-basin "same step" equality as a hard failure (the milestones.md
  prose's "consistent across basins / same step" clause is **not** spec-supported and is
  removed). If cross-basin step consistency is reported at all, it is the **FIRST R3
  skip-with-reason**, **never** a hard fail. M6 **never** interprets the cadence *word*
  (it never asserts `"daily"` == 1-day spacing — that is semantic interpretation HDX must
  avoid). The documented limit: **HDX verifies axis REGULARITY, not that spacing matches
  the cadence word.** If axis regularity cannot be determined cheaply for a basin/fixture
  (e.g. the only cheap signal is a two-point `[start,end]` extent, from which a single
  step is not derivable), M6 is reported **SKIPPED-with-reason** under R3 rather than
  silently passing. The no-cadence-semantics tension is documented in M6's doc comment.
  The MS8 M6 negative fixture's expectation must match **this** rule (an irregular
  per-basin axis fails M6; a merely-different-cross-basin-step dataset does **not**).

  **Cheap regularity signal available in v0.1.** The discovery layer surfaces a per-basin
  `[start,end]` extent + a `sorted_ascending` flag, **not** the full 1-D `time` array. A
  two-point extent cannot prove a *constant interior step*. So for the MS2 fixture M6's
  rule (b) is honestly **R3 SKIPPED-with-reason** ("per-basin axis regularity needs the
  full 1-D time array; v0.1 discovery surfaces only `[start,end]` + sortedness — byte-deep
  axis-regularity verification is deferred"); M6's rule (a) (cadence non-empty) is **ran:
  pass**. The dataset is still `conformant:true` because a SKIPPED check is not a FAIL
  (fail-closed applies only to a violated MUST that **ran**). This is the documented,
  honest outcome — the safety valve the milestones.md M6 risk section calls for. (If a
  future step surfaces the full per-basin `time` array as a 1-D index read, rule (b) can
  graduate from R3-skip to ran; that is a later change, recorded as an amendment.)

**The R3 depth + ran/skipped discipline (FOLD: entry discipline + honesty).** Every
check records (i) **ran vs skipped**, (ii) its **R3 depth class** (`MetadataDeep` vs
`ByteDeep`), and (iii) for a skip, a **reason string**. The `ValidationReport` clearly
reports which checks ran (spec §14 note). No check decodes a gridded chunk or pixel
raster (LOW-3); `validate` runs over the discovery layer + the 1-D index reads MS3/MS4
already do. The honest byte-deep items in v0.1: **M6 rule (b)** (per-basin axis
regularity — needs the full 1-D `time` array → R3 skip on the two-point extent), and the
**§8 sharding / consolidated-metadata internals** (already classified at MS4; surfaced
in the report as a note, not a separate failing check). Everything else is
metadata-deep and **live** on the MS2 fixture.

---

## Ordering rationale

MS6 turns the proven discovery layer + `describe` into the conformance verb. Three
dependency-sequential steps, each one conventional commit leaving the tree green
(`cargo build` + `cargo test` + `cargo clippy --all-targets -- -D warnings`) with the
mandated bump+tag:

1. **S1 — Report types + the rule-engine skeleton + the in-memory-falsifiable checks
   (no IO beyond the manifest entry-gate).** Stand up `CheckId` (the 19 §14 ids),
   `CheckOutcome` (id + ran/skipped + pass/fail + R3 depth class + detail/reason),
   `ValidationReport` (`Vec<CheckOutcome>` + `conformant: bool`), and the verb skeleton
   `validate(path)` whose **first** act mirrors `describe`'s §0 entry gate (read
   `manifest.json` → `Manifest::from_json` hard-cut **before** discovery). S1 implements
   every check whose rule is a **pure function over the already-typed model** and can be
   **falsified in-memory without differently-shaped on-disk bytes** (FOLD MED-2): **H1,
   H2, I3, M3, M4, T1, G1**, plus the trivially-pure **M1, M2** (the entry gate
   surfaces them as outcomes). Each such check ships with a **mandatory in-memory
   negative unit test** (e.g. a hand-built `Vec<Field>` with a dtype mismatch falsifies
   H1; a duplicated `BasinId` falsifies I3; a nullable `TimeColumn` falsifies T1). This
   freezes the report shape + the rule-function surface **before** the cross-file checks,
   so S2 builds against a settled contract. The verb at end of S1 already returns a
   well-formed (partial) report and a defensible `conformant` over the in-memory rules.

2. **S2 — The cross-file / cross-basin checks + the full §14 wiring + the
   fixture-backed verdicts.** Implement the checks whose rule needs the **full discovery
   layer assembled from on-disk bytes**: **L1, L2, L3** (layout), **I1, I2** (identity vs
   folder/columns), **M5** (manifest CRS vs every file's recorded CRS), **M6** (rule (a)
   ran + rule (b) R3-skip per the fold), **T2** (intra-basin axis identity, gaps
   NaN-filled), **G2** (shared label ⇒ cell-for-cell alignment — positive path on the
   shared aligned fixture label), **G3** (CF / GeoTIFF georef present), **Geo1**
   (outlines schema/label — surfaced from MS4's read). Wire the reserved `CoreError`
   variants (`MissingRootRollup`, `BasinIdFolderMismatch`, `RaggedSchema`,
   `GridLabelMismatchAcrossBasins`, `NonMonotonicTime`) as the detail vocabulary the
   failing outcomes reference. Prove the **three milestone verdicts**: `conformant:true`
   on `valid/minimal` (every applicable check `ran:pass` or honestly `skipped`,
   **including G2's positive path** on the shared aligned `era5` label), and
   `conformant:false` on **both** MS2 invalids (`wrong-format-version` → M2 fails;
   `missing-root-rollup` → L1 fails), each pinning exactly its one check. Where a
   cross-file check **can** still be falsified in-memory over the typed model (rare; e.g.
   M5 with a hand-built `GridInfo` whose `Crs` differs from a hand-built `Manifest`'s),
   a unit test does so and states it per check; otherwise the exhaustive on-disk negative
   is **explicitly deferred to MS8** (the same R2 rationale).

3. **S3 — `ValidationReport` JSON serialization + `validate.schema.json` + golden report
   + jsonschema/snapshot tests (the report wire shape pinned).** Add the describe-local
   `#[derive(Serialize)]` DTO for `ValidationReport` (the inert domain types stay free of
   `serde::Serialize`, mirroring MS5's DTO discipline), a `validate_json(path) ->
   Result<String, ValidateError>` boundary, `schemas/validate.schema.json`
   (`additionalProperties:false`), the committed golden report for `valid/minimal`, and
   the `jsonschema` + snapshot tests. This locks the report shape (the CLI MS7 + PyO3 MS9
   consume it) and proves the report **clearly reports which checks ran** (spec §14
   note) as a machine-readable artifact.

This order (report shape + pure rules → cross-file rules + verdicts → wire-shape lock)
mirrors the MS5 discipline (shape → verb → contract-lock) and keeps each commit
independently reviewable and green.

---

## Scope guard (read before every step)

- **No step exceeds MS6 or does a later milestone's work.** `validate` implements the
  §14 checklist and emits a `ValidationReport`; it is **MS7** that wraps it in the CLI
  (exit codes), **MS8** that builds the exhaustive one-violation-per-check invalid
  family + golden regression matrix, and **MS9** that mirrors it in PyO3. MS6 ships **no**
  `main.rs` change, **no** new fixtures beyond reusing the three MS2 trees, and **no**
  PyO3.
- **No regrid / clip / reduce**, ever. `validate` adds **no reader** and decodes **no
  gridded chunk / pixel raster** (LOW-3) — it consumes `discover()`'s output + the 1-D
  index reads MS3/MS4 already do.
- **Inert / agnostic discipline holds.** No new type or field carries transform, role,
  semantic type, or provenance. The `ValidationReport` carries only check ids, ran/skip,
  pass/fail, an R3 depth class, and an opaque detail/reason string — never a derived
  domain field. The manifest stays exactly six fields; `format_version` is a hard cut.
- **No cadence-word interpretation; no cross-basin time-extent equality.** M6 verifies
  axis *regularity* only (rule (a) cadence-non-empty + rule (b) per-basin uniform spacing,
  the latter R3-skipped in v0.1); it never reads `"daily"` as a step, and it never fails a
  dataset for ragged per-basin extents (§6.1). Any cross-basin step *consistency* report
  is at most the **first R3 skip-with-reason**, never a hard fail.
- **Fail-closed, but only on a MUST that RAN.** A violated MUST that **ran** ⇒
  `conformant:false`. A **skipped** check (honest R3 skip with a reason) does **not** by
  itself make a dataset non-conformant — it is reported as `skipped`, and the report
  states which checks ran. A *structural* failure (unreadable dir, undecodable present
  artifact, unreadable manifest, the §0 hard cut) returns a typed `Err`, **not** a
  `conformant:false` report (mirrors `describe`: the version hard cut is an `Err`, not a
  check outcome — see the entry-gate note in S1).
- **Do not reshape the discovery layer.** `Discovery` / `ScalarDiscovery` /
  `GriddedDiscovery` / `Field` / `GridInfo` / `TimeExtent` / `TimeColumn` are MS3/MS4
  contracts; MS6 reads through their existing accessors. The **one** permitted additive
  seam (S2) is surfacing `scalar_static`'s `has_basin_id` for I1 if it is not already
  reachable — see the S2 seam note; it is an **additive accessor**, never a reshape, and
  if it cannot be added without reshaping, I1's static-rollup leg is an honest R3 skip.

---

## S1 — Report types + rule-engine skeleton + the in-memory-falsifiable checks

**id.** MS6-S1

**Intent.** Freeze the `ValidationReport` wire shape and the per-check rule-function
surface **before** the cross-file checks exist (the same types-first discipline as
MS5-S1), and implement every §14 check that is a **pure function over the already-typed
discovery model and can be falsified in-memory without differently-shaped on-disk bytes**
(FOLD MED-2): **H1, H2, I3, M3, M4, T1, G1** (plus the entry-gate **M1, M2**). Each ships
a **mandatory in-memory negative unit test**. Stand up the verb skeleton `validate(path)`
whose first act is the §0 hard-cut entry gate (mirroring `describe`). The verb at end of
S1 returns a well-formed (partial) `ValidationReport`; the cross-file checks land in S2.
Zero new readers; the only IO is the manifest read + `discover()` already used by
`describe`.

**Changes.**
- `crates/core/src/validate.rs` (new module; `pub mod validate;` added to `lib.rs` with
  a `//!`-referenced entry in the module map).
- In `validate.rs`:
  - `pub enum CheckId { M1, M2, M3, M4, M5, M6, L1, L2, L3, I1, I2, I3, H1, H2, T1, T2,
    G1, G2, G3, Geo1 }` — the 19 §14 ids as an enum (never strings) with `as_str()` →
    the stable lowercase/spec id (`"M1"`…`"Geo1"`) for the wire shape; `#[derive(Debug,
    Clone, Copy, PartialEq, Eq)]`.
  - `pub enum CheckStatus { Ran, Skipped }` and `pub enum CheckResult { Pass, Fail }`
    (enums over booleans, architecture §3.3) and `pub enum DepthClass { MetadataDeep,
    ByteDeep }` (the R3 classification).
  - `pub struct CheckOutcome { id: CheckId, status: CheckStatus, result: Option<CheckResult>,
    depth: DepthClass, detail: Option<String> }` — fields private + getters; a `Skipped`
    check carries `result: None` + a `detail` reason; a `Ran` check carries
    `Some(Pass|Fail)` + an optional detail. Constructor helpers `ran_pass(id, depth)`,
    `ran_fail(id, depth, detail)`, `skipped(id, depth, reason)`. `#[derive(Debug, Clone,
    PartialEq, Eq)]`.
  - `pub struct ValidationReport { checks: Vec<CheckOutcome>, conformant: bool }` — fields
    private + getters (`checks()`, `conformant()`); `conformant` is computed as
    "**no check that `Ran` has `result == Fail`**" (a `Skipped` check never flips it).
    A `find(id) -> Option<&CheckOutcome>` accessor for tests/CLI. `#[derive(Debug, Clone,
    PartialEq, Eq)]`.
  - The **pure rule functions** (one per in-memory check), each taking borrowed pieces of
    the typed model and returning a `CheckOutcome`, each doc-commented with the spec rule
    it enforces and its R3 depth class:
    - `check_m3 / check_m4` — folded into the entry-gate: because `Manifest::from_json`
      already rejects a 7th/missing field (M3) and a bad `created_at`/empty `crs`/empty
      `cadence` (M4) at the boundary, S1 records M3/M4 as `ran:pass` once the manifest
      parses, and the entry gate (below) maps a manifest parse error to the right
      failing/`Err` outcome. (M3/M4 negatives are exercised in-memory by calling
      `Manifest::from_json` on a hand-built 7-field / empty-crs JSON string — no on-disk
      bytes needed.)
    - `check_h1(fields_by_basin: &[(&BasinId, Vec<&Field>)]) -> CheckOutcome` — the field
      schema (names, dtypes, quadrants, grid_label) is **identical across basins**; a
      divergent basin ⇒ `ran:fail` with a `RaggedSchema`-style detail. `MetadataDeep`.
    - `check_h2(labels_by_basin) -> CheckOutcome` — the grid-label **set** is identical
      across basins; a divergent set ⇒ `ran:fail` (`GridLabelMismatchAcrossBasins`
      detail). `MetadataDeep`.
    - `check_i3(in_file_ids: &[&BasinId]) -> CheckOutcome` — `basin_id` is **unique**
      within the dataset; a duplicate ⇒ `ran:fail`. `MetadataDeep`.
    - `check_t1(per_basin_time: &[(&BasinId, Option<&TimeColumn>)]) -> CheckOutcome` —
      every present `time` column is named `time`, dtype `Timestamp`, **non-nullable**,
      **sorted ascending**; any violation ⇒ `ran:fail` (`NonMonotonicTime` detail for the
      sort leg). `MetadataDeep`.
    - `check_g1(gridded_fields: &[&Field]) -> CheckOutcome` — every gridded field
      **self-names** (carries `Some(GridLabel)` — the `Field::new` invariant already
      guarantees gridded ⇒ label, so G1 verifies the catalog is built that way and there
      is no positional channel axis); `ran:pass` on the fixture. `MetadataDeep`.
  - `validate(path: impl AsRef<Path>) -> Result<ValidationReport, ValidateError>` — the
    verb skeleton, `#[instrument]`, `tracing` milestones. Order mirrors `describe`:
    (1) read `<path>/manifest.json` (IO error → `ValidateError::ManifestUnreadable`);
    (2) `Manifest::from_json` — **the §0 hard cut + six-field parse runs FIRST, before any
    discovery**: an unknown `format_version` returns `Err(ValidateError::Manifest(
    CoreError::UnknownFormatVersion{..}))` (M1/M2 as the entry gate, fail-closed); a
    malformed manifest (extra/missing field, bad timestamp, empty crs/cadence) likewise
    returns `Err` (these are structural manifest failures, not check outcomes — mirrors
    `describe`). (3) `discover(path)` (structural failure → `ValidateError::Discovery`).
    (4) Build the report by running the S1 in-memory checks over the assembled model,
    pushing one `CheckOutcome` per check; the cross-file checks (S2) push placeholder
    `skipped`-with-reason outcomes in S1 so the report already lists all 19 ids
    (replaced by real outcomes in S2) — **or** S1 simply pushes only the checks it owns
    and S2 appends; the chosen approach is documented in S1 (recommended: list all 19,
    cross-file ones `skipped("not yet wired")` in S1, so the report shape is complete and
    S2 only flips skips to ran).
  - `crates/core/src/error.rs` — add `ValidateError` mirroring `DescribeError`
    (named-field variants, each doc-commented with *when* it fires):
    `ManifestUnreadable{path,detail}`, `Manifest(#[source] CoreError)` (wraps the §0 hard
    cut + the malformed-manifest cases), `Discovery(#[source] CoreError)`, and a
    `Serialize{detail}` reserved for S3. **No `unwrap`/`expect`/panic** in library code.
    Decision recorded: a violated MUST is a **report outcome**, never a `ValidateError`;
    `ValidateError` is only for structural/entry failures.
  - `lib.rs` module-map doc updated to describe the verb's entry order + the
    report-vs-error split.
- `crates/core/README.md` — add `validate` + `ValidationReport` to the Mermaid module map
  + glossary rows (`CheckId`, `CheckOutcome`, `ValidationReport`, `R3 depth class`,
  `ran/skipped`).
- `Cargo.toml` (root) — `./scripts/bump-version.sh patch`; stage alongside code.

**Test plan.**
- **Entry gate (FOLD entry discipline):** `validate(conformance("invalid/wrong-format-version"))`
  returns `Err(ValidateError::Manifest(CoreError::UnknownFormatVersion{found:"0.2"}))`
  — the §0 hard cut runs **before** discovery (statically guaranteed by the function
  order, mirroring `describe`'s proven test); a temp dir with **no** `manifest.json`
  returns `ValidateError::ManifestUnreadable`.
- **H1 in-memory negative (MANDATORY, FOLD MED-2):** a hand-built two-basin
  `Vec<(&BasinId, Vec<&Field>)>` where one basin's field has a **dtype mismatch** (e.g.
  `streamflow: F64` vs `F32`) ⇒ `check_h1` returns `ran:fail`; the matching pair ⇒
  `ran:pass`.
- **H2 in-memory negative (MANDATORY):** one basin's grid-label set `{era5}` vs another's
  `{era5, chirps}` ⇒ `check_h2` `ran:fail`; identical sets ⇒ `ran:pass`.
- **I3 in-memory negative (MANDATORY):** a `Vec<&BasinId>` with a duplicate `0001` ⇒
  `check_i3` `ran:fail`; all-distinct ⇒ `ran:pass`.
- **M3 / M4 in-memory negatives (MANDATORY):** `Manifest::from_json` on a hand-built
  7-field JSON ⇒ the entry gate maps to the M3-failing path; on an empty-`crs` /
  bad-`created_at` JSON ⇒ the M4 path. (Exercised via the verb's entry gate or the
  rule directly — no on-disk bytes.)
- **T1 in-memory negative (MANDATORY):** a hand-built `TimeColumn` that is **nullable**
  (or **not** sorted ascending, or dtype ≠ `Timestamp`, or name ≠ `time`) ⇒ `check_t1`
  `ran:fail` for that leg; the conformant descriptor ⇒ `ran:pass`.
- **G1 in-memory negative (MANDATORY):** since `Field::new` makes a label-less gridded
  field unrepresentable, G1's negative is exercised at the **rule input** level: a
  catalog where a "gridded" entry is mis-modeled (constructed via the test-only path with
  `grid_label: None` is impossible, so the negative asserts the rule **rejects** a field
  list that claims a positional channel axis / a gridded field missing its label by
  feeding the rule a hand-built mixed list and asserting `ran:pass` only when every
  gridded field self-names). Documented per FOLD MED-2 as the in-memory-falsifiable form.
- **Report shape:** the S1 partial report over `valid/minimal` lists **all 19** check
  ids (the S1-owned ones `ran`, the S2 ones `skipped("not yet wired")`), and
  `conformant()` is computed as "no ran-fail" (so a partial report with only passing ran
  checks is `true`).
- **No verdict-flipping by skips:** a report with one `skipped` check and otherwise all
  `ran:pass` has `conformant == true`.

**Acceptance.**
- `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings` all pass.
- `CheckId` / `CheckOutcome` / `ValidationReport` / `ValidateError` compile; the inert
  domain types gain **no** `serde::Serialize` derive (S3 owns the wire shape) and **no**
  new inert-violating field; the report carries only id / ran-skip / pass-fail / depth /
  opaque detail.
- `validate` performs the **§0 hard version cut + manifest boundary-parse FIRST** (entry
  gate); a test confirms it `Err`s `UnknownFormatVersion` on `format_version:"0.2"`
  **before** discovery (advances spec-checks **M1, M2** as the fail-closed entry gate).
- The in-memory-falsifiable checks **H1, H2, I3, M3, M4, T1, G1** are **implemented** with
  a **mandatory in-memory negative unit test each** (FOLD MED-2); each records its R3
  depth class (`MetadataDeep`).
- `conformant` is "no check that **ran** failed"; a `skipped` check never flips it.
- No `unwrap`/`expect`/panic in the new library code; every failure is a typed error or a
  recorded check outcome.
- Commit via `./scripts/bump-version.sh patch` + stage `Cargo.toml` + conventional commit
  + `git tag v<version>`.

**Spec refs.** §0 (hard cut first), §1 (inert/agnostic — no semantic field on the
report), §2 (field 2×2 / quadrant — H1, G1), §3 (basin_id uniqueness — I3), §5
(homogeneity — H1, H2), §6/§6.3 (time descriptor — T1), §8 (one artifact = one grid,
self-naming — G1), §10 (validate = the spec executed), §11 (six-field floor — M3/M4),
§14 M1–M4, H1, H2, I3, T1, G1, §14 note (report which checks ran); architecture §1, §3.5,
§5, §7 R3.

**Commit message.** `feat(core): add ValidationReport types, validate entry gate, and in-memory §14 checks (H1/H2/I3/M3/M4/T1/G1)`

---

## S2 — Cross-file / cross-basin checks + full §14 wiring + fixture-backed verdicts

**id.** MS6-S2

**Intent.** Complete the §14 checklist with the checks whose rule needs the **full
discovery layer assembled from on-disk bytes** — **L1, L2, L3, I1, I2, M5, M6, T2, G2,
G3, Geo1** — wire the reserved `CoreError` variants as the failing outcomes' detail
vocabulary, and prove the **three milestone verdicts**: `conformant:true` on
`valid/minimal` (incl. spec-check **G2's positive path** on the shared aligned `era5`
label) and `conformant:false` on **both** MS2 invalids. Implement the **M6 cadence rule
exactly as the FOLD MED-1 specifies** (rule (a) cadence-non-empty ran:pass; rule (b)
per-basin axis regularity R3-skipped-with-reason in v0.1; **no** cross-basin equality,
**no** cadence-word interpretation). Every check ends up either `ran` (pass/fail) or
honestly `skipped` with a reason; the report states which ran. Independently committable
on S1; green.

**Changes.**
- `crates/core/src/validate.rs` — add the cross-file rule functions + flip the S1
  placeholders to real outcomes:
  - `check_l1(root_rollups) -> CheckOutcome` — both `scalar_static.parquet` and
    `outlines.geoparquet` present at root; an absent rollup ⇒ `ran:fail`
    (`MissingRootRollup{artifact}` detail). `MetadataDeep`.
  - `check_l2(layout-derived) -> CheckOutcome` — each `basin=<id>` dir has
    `scalar_dynamic.parquet`, and a `gridded_static/` / `gridded_dynamic/` subtree
    **iff** the field schema declares gridded fields (derive "required artifacts" from
    the field set, not a fixed mode — spec §2 / architecture §3.3). Missing required
    artifact, or a gridded subtree present with **no** gridded fields ⇒ `ran:fail`.
    `MetadataDeep`. (The needed presence facts come from the layout model already inside
    `discover`; S2 surfaces them via the existing `root_rollups()` / per-basin gridded
    artifact accessors. If a layout fact is not reachable through the current accessors,
    add an **additive accessor** on the discovery model — never a reshape — and document
    it; if it cannot be surfaced additively, L2/L3 are honest R3 skips with a reason.)
  - `check_l3(...) -> CheckOutcome` — no stray/ragged HDX files; absence of a field is
    NaN, never a missing file (§5). Implemented over the structural facts the walk
    already records (the walk already ignores dot-cruft, pre-empting false positives,
    `layout.rs::is_ignored_entry`). On the conformant fixture ⇒ `ran:pass`. `MetadataDeep`.
  - `check_i1(...) -> CheckOutcome` — `basin_id` is a real column in `scalar_static`,
    every `scalar_dynamic`, and `outlines`. The per-basin `scalar_dynamic` leg uses
    `basin_id_in_file().is_some()`; the `outlines` leg uses MS4's read (the geoparquet
    reader already requires `basin_id`/`delineation`/`geometry` and errors otherwise —
    so a present-and-readable outlines satisfies the column-presence leg, and an absent
    outlines is an L1 fail, not an I1 fail). **Seam note:** `scalar_static`'s
    `has_basin_id` is read inside `discover_scalar` but **not currently surfaced** on
    `ScalarDiscovery`; S2 adds an **additive accessor** (e.g.
    `ScalarDiscovery::scalar_static_has_basin_id() -> Option<bool>`) to expose it
    (additive — no reshape). If that accessor cannot be added cleanly, I1's static-rollup
    leg is an honest **R3 skip-with-reason** and the per-basin leg still runs. `MetadataDeep`.
  - `check_i2(per_basin) -> CheckOutcome` — in-file `basin_id` agrees with the
    `basin=<id>` folder id, per basin; a disagreement ⇒ `ran:fail`
    (`BasinIdFolderMismatch{in_file,folder}` detail). `MetadataDeep`.
  - `check_m5(manifest_crs, grids) -> CheckOutcome` — the manifest `crs` matches the
    `Crs` recorded on **every** `GridInfo` (and the outlines CRS via MS4's
    `OutlinesInfo::crs()`); a mismatch ⇒ `ran:fail`. On the fixture all are `EPSG:4326`
    ⇒ `ran:pass`. **R3 note:** a file whose CRS could not be resolved to an `EPSG:<code>`
    (recorded raw by MS4's CRS-recording rule) makes M5 for that file an honest
    `skipped`-with-reason, never a silent pass. `MetadataDeep`.
  - `check_m6(manifest_cadence, per_basin_extents) -> CheckOutcome` — **FOLD MED-1, the
    most important fold.** Rule (a): `cadence` is a non-empty string ⇒ contributes
    `ran:pass` (references M4). Rule (b): per-basin axis **regularity** (uniform interior
    spacing) — but v0.1 discovery surfaces only a two-point `[start,end]` extent +
    `sorted_ascending`, from which a constant interior step is **not** derivable, so rule
    (b) is **`skipped`** with the reason *"per-basin axis regularity needs the full 1-D
    time array; v0.1 discovery surfaces only [start,end] + sortedness — byte-deep
    axis-regularity verification deferred"*, classified **`ByteDeep`**. M6's overall
    outcome on the fixture: `ran:pass` for the non-empty-cadence leg with a recorded
    `skipped` note for the regularity leg (a single `CheckOutcome` for M6 carries the
    honest combined status: `Ran`/`Pass` with a detail naming the R3-skipped regularity
    leg, **or** — chosen and documented in S2 — `Skipped` with the regularity reason; the
    rule **never** interprets the cadence word and **never** asserts cross-basin step
    equality). The doc comment states the limit verbatim (REGULARITY, not the cadence
    word) and that any cross-basin step report would be the **first R3 skip-with-reason**,
    never a hard fail.
  - `check_t2(per_basin scalar + gridded) -> CheckOutcome` — within each basin, the
    `scalar_dynamic` `time` axis and every `gridded_dynamic` artifact share the
    **identical** time axis (§6.2; gaps NaN-filled). v0.1 surfaces the scalar `[start,end]`
    extent + the Zarr per-grid metadata; the **cheap** T2 leg compares the per-basin
    scalar extent against the Zarr `time` coordinate metadata where reachable, else is an
    honest **R3 skip-with-reason** (cross-artifact full-axis identity is the genuinely
    on-disk-shape-dependent negative reserved for MS8). On the fixture ⇒ `ran:pass` or
    `skipped` (documented). `MetadataDeep`/`ByteDeep` per the leg.
  - `check_g2(per_basin gridded) -> CheckOutcome` — **its positive path is exercised on
    the MS2 valid fixture** (critique H-1): when a label appears in **both** the
    `gridded_static` (COG) and `gridded_dynamic` (Zarr) subtrees, the two `GridInfo`
    extents (and resolution + dims) **MUST coincide** — on the fixture both are `era5` and
    coincide at `10.0/50.0/11.5/48.0`, 6×8 ⇒ `ran:pass`. A shared-but-misaligned label ⇒
    `ran:fail` (the MS8 negative). `MetadataDeep`. (Reuses the already-tested
    `BasinGridded::static_grid_labels()` / `dynamic_grid_labels()` + `grid_info()`.)
  - `check_g3(grids) -> CheckOutcome` — every grid carries resolvable CF / GeoTIFF
    georef (the readers already error `MissingGridGeoref` if absent, so a present
    discovered `GridInfo` with a recorded `Crs` satisfies G3); ⇒ `ran:pass` on the
    fixture. `MetadataDeep`.
  - `check_geo1(...) -> CheckOutcome` — `outlines.geoparquet` has rows
    `(basin_id, delineation, geometry)`, the label column is `delineation`, not
    partitioned by delineation. MS4's geoparquet reader already enforces the schema
    columns (erroring `MissingGeometryColumn` otherwise) and reads a single root file
    (not partitioned), so a present-and-read outlines satisfies Geo1 ⇒ `ran:pass`; an
    absent outlines is an **L1** fail (not Geo1). `MetadataDeep`.
  - Replace the S1 `skipped("not yet wired")` placeholders for these ids with their real
    outcomes; the final report lists all 19 ids, each `ran`(pass/fail) or honestly
    `skipped` with a reason.
- `crates/core/src/discovery.rs` and/or `gridded_discovery.rs` — **only if needed**, an
  **additive accessor** (the I1 static-rollup seam; possibly an L2/L3 layout-fact
  accessor), documented as additive, never a reshape. No existing accessor/type changes.

**Test plan.**
- **`conformant:true` on the valid fixture (FOLD; the milestone's positive proof):**
  `validate(conformance("valid/minimal"))` is `Ok`; the report has `conformant() == true`;
  **every** applicable check is `ran:pass` **or** honestly `skipped` with a reason
  (assert no check is `ran:fail`); the report lists all 19 ids (assert the id set).
- **G2 positive path fired (FOLD critique H-1):** assert the report's `G2` outcome is
  `ran:pass` and (via the model) that the COG + Zarr `era5` extents coincided at
  `10.0/50.0/11.5/48.0` — the shared aligned grid label proves G2's positive path on its
  own milestone.
- **`conformant:false` on `wrong-format-version` (FOLD critique H-3) — entry gate:**
  `validate(conformance("invalid/wrong-format-version"))` returns `Err(Manifest(
  UnknownFormatVersion))` (the §0 hard cut wins before discovery), **or** — if S2 chooses
  to surface M2 as a fail outcome instead of an `Err` — a report with `M2` `ran:fail` and
  `conformant() == false`. The chosen behavior is documented and consistent with
  `describe`'s (recommended: keep the §0 hard cut an `Err`, matching `describe`, and note
  that the CLI MS7 maps it to exit code 2; the `conformant:false` *outcome* observable for
  M2 is then proven by an in-memory hand-built manifest with a wrong version fed through
  the M2 rule, **plus** the MS8 on-disk negative). The test asserts the observable
  consequence either way: the wrong-version tree never reports `conformant:true`.
- **`conformant:false` on `missing-root-rollup` (FOLD critique H-3) — L1:**
  `validate(conformance("invalid/missing-root-rollup"))` is `Ok` with a report where `L1`
  is `ran:fail` (the absent `outlines.geoparquet`), `conformant() == false`, and **every
  other** check is `ran:pass` or honestly `skipped` (assert exactly L1 fails — the one
  pinned check, per the one-violation discipline).
- **M6 rule (FOLD MED-1):** assert M6 on the valid fixture is **not** `ran:fail`; assert
  its detail/reason names **axis regularity** (rule (b)) as the R3-skipped leg and that
  rule (a) (cadence non-empty) passed; assert M6 does **not** reference the cadence word
  or a cross-basin step; a unit test with a hand-built non-empty cadence + ragged-but-each-
  internally-unknown extents confirms M6 never fails for ragged extents (§6.1).
- **M5 in-memory falsifiable leg:** a hand-built `Manifest` (`crs:"EPSG:4326"`) vs a
  hand-built `GridInfo` with `Crs::new("EPSG:3857")` ⇒ `check_m5` `ran:fail`; matching ⇒
  `ran:pass` (the in-memory-falsifiable form; the on-disk crs-mismatch negative is MS8).
- **I2 in-memory falsifiable leg:** a hand-built `BasinScalar`-style pair with
  `folder="0001"`, `in_file="9999"` ⇒ `check_i2` `ran:fail`; matching ⇒ `ran:pass` (the
  on-disk folder-mismatch negative is MS8).
- **Deferral statement (FOLD MED-2):** a test-module comment enumerates the genuinely
  on-disk-shape-dependent negatives reserved for MS8 (I2-on-disk folder mismatch, T2
  cross-artifact axis mismatch, G2 misaligned-shared-grid, G3 missing georef, L1/L2/L3
  layout mutations, Geo1 column/partition, M5 file crs-mismatch) and states MS6 makes **no
  claim** of an on-disk negative for those — they are the MS8 matrix.
- **Companion-mask / `{source}_{variable}` ordinariness:** assert `validate` treats
  `era5_precipitation` and `era5_precipitation_was_filled` as **ordinary** fields — H1/G1
  apply no suffix/prefix special-casing (the report's `conformant:true` already implies
  it; an explicit assertion pins it).

**Acceptance.**
- `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings` all pass.
- **All §14 checks are IMPLEMENTED** (M1–M6, L1–L3, I1–I3, H1–H2, T1–T2, G1–G3, Geo1):
  each is `ran` (pass/fail) **or** honestly reported `skipped` with a reason under R3; the
  report states which ran (FOLD: reworded exit criterion — IMPLEMENTS, not "enforces, with
  on-disk negatives" — the on-disk matrix is MS8).
- **spec-check M6** has the **defined non-semantic rule** of FOLD MED-1: cadence-non-empty
  (ran) + per-basin axis regularity (R3-skip-with-reason in v0.1); **no** cross-basin
  equality, **no** cadence-word interpretation; documented in its doc comment.
- The MS2 valid fixture validates **`conformant: true`** and **G2's positive path is
  exercised** on it (shared aligned `era5` label); **both** MS2 invalids validate
  **`conformant: false`**, each pinning its one check (M2/entry-gate; L1).
- Every check records ran-vs-skipped + its R3 depth class; no check decodes a gridded
  chunk or pixel raster (LOW-3 — reused metadata-only discovery + 1-D index reads).
- Any new discovery accessor is **additive** (documented as such), never a reshape of the
  MS3/MS4 contracts.
- No `unwrap`/`expect`/panic in the new library code.
- Commit via the bump+tag convention.

**Spec refs.** §0 (hard cut first), §1 (inert/agnostic — **no** cadence-word
interpretation; §6.1 ragged extents not failed), §2 (ordinary fields; gridded subtrees
derived from the field set — L2), §3 (basin_id authority/folder/uniqueness — I1, I2),
§4 (layout — L1, L2, L3), §5 (homogeneity; absence=NaN not missing file — L3), §6.2
(intra-basin shared time axis — T2), §6.4 (cadence is a validated convention, not
interpreted — M6), §7 (per-grid georef, one dataset-wide CRS — M5, G3), §8 (one
artifact=one grid; shared label ⇒ alignment — G2), §9 (plural outlines, `delineation`
column, not partitioned — Geo1), §10 (validate = spec executed), §11 (manifest floor),
§14 (every check id), §14 note (report which checks ran); architecture §1, §5, §7 R3.

**Commit message.** `feat(core): wire the cross-file §14 checks (L*/I1/I2/M5/M6/T2/G2/G3/Geo1) and prove conformant verdicts`

---

## S3 — `ValidationReport` JSON + `validate.schema.json` + golden report (wire shape pinned)

**id.** MS6-S3

**Intent.** Lock the `ValidationReport` wire shape (the CLI MS7 + PyO3 MS9 consume it):
add a describe-local `#[derive(Serialize)]` DTO for the report (keeping the inert domain
types free of `serde::Serialize`, mirroring MS5's DTO discipline), a
`validate_json(path) -> Result<String, ValidateError>` boundary, pin
`schemas/validate.schema.json` (`additionalProperties:false`), commit the golden report
for `valid/minimal`, and assert both (a) the golden validates against the schema via the
`jsonschema` dev-dep and (b) `validate` of the valid fixture equals the golden as parsed
JSON. This makes the spec §14-note requirement ("the validator MUST clearly report which
checks ran") a machine-readable, pinned artifact. Independently committable on S2; green.

**Changes.**
- `crates/core/src/validate.rs` — add:
  - private `#[derive(Serialize)]` DTOs: `ValidationReportDto { checks: Vec<CheckOutcomeDto>,
    conformant: bool }` and `CheckOutcomeDto { id: &str, status: &str ("ran"/"skipped"),
    result: Option<&str> ("pass"/"fail"), depth: &str ("metadata_deep"/"byte_deep"),
    detail: Option<&str> }`. Each DTO field doc-named with its single source. The inert
    `CheckOutcome` / `ValidationReport` gain **no** `serde::Serialize` derive.
  - `ValidationReport::to_dto(&self) -> ValidationReportDto`, `to_json_string` /
    `to_json_pretty`.
  - `pub fn validate_json(path) -> Result<String, ValidateError>` — `validate(path)` +
    `to_json_string`, mapping a (practically unreachable) serialize failure to
    `ValidateError::Serialize{detail}`.
- `schemas/validate.schema.json` (new) — the report shape: top-level object with required
  `{checks, conformant}`, `additionalProperties:false`; `checks` an array of objects
  `{id (enum of the 19 ids), status (enum ran|skipped), result (enum pass|fail|null),
  depth (enum metadata_deep|byte_deep), detail (string|null)}`; `conformant` boolean.
  Title/description cross-referencing spec §10/§14 + the §14-note ran/skip requirement;
  shape versioned implicitly by `format_version` only.
- `conformance/valid/minimal/validate.golden.json` (path chosen + documented in
  `conformance/README.md`, mirroring the describe golden) — the committed golden report,
  produced by the S2 verb (pretty-printed, deterministic order).
- `conformance/README.md` — add a "golden validate output" subsection: where it lives,
  that it is produced by `hdx-core`'s `validate` (not the Python generator), the
  golden-update workflow note (MS8 extends it), and that the report records which checks
  ran/skipped (spec §14 note).
- `crates/core/src/validate.rs` (tests module) — the schema + snapshot tests.

**Test plan.**
- **R4-style schema test (jsonschema dev-dep):** compile `schemas/validate.schema.json`;
  assert the **golden** `validate` report of `valid/minimal` **validates** against it.
- **Golden snapshot test:** `validate_json(conformance("valid/minimal"))` parsed to a
  `serde_json::Value` equals the committed golden parsed to a `Value` (compare as parsed
  JSON). A regeneration comment documents how to refresh the golden (on a deliberate shape
  change only).
- **Report-states-which-ran (FOLD honesty / §14 note):** assert the golden's `checks`
  array contains all 19 ids; assert at least the v0.1 honest skips (M6 regularity leg, and
  any T2/M5/I1 legs S2 classified as skips) appear with `status:"skipped"` + a non-empty
  `detail` reason; assert every other check is `status:"ran"` with `result:"pass"`; assert
  top-level `conformant: true`.
- **Negative schema test:** a hand-mutated golden with an injected extra top-level key (or
  a check object missing `id`/with an unknown `status`) **fails** schema validation
  (`additionalProperties:false` / enum constraints work), proving the schema catches a
  shape drift.
- **Both invalids' reports serialize:** `validate_json` over each MS2 invalid produces
  valid JSON; the `missing-root-rollup` report serializes with `conformant:false` + `L1`
  `result:"fail"` (a smoke test that the wire shape carries a fail outcome correctly; the
  exhaustive per-check golden matrix is MS8).

**Acceptance.**
- `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings` all pass.
- `schemas/validate.schema.json` committed; the golden report committed; both asserted in
  Rust tests via the `jsonschema` dev-dep + a snapshot equality test.
- The DTO owns the wire shape; the inert `CheckOutcome` / `ValidationReport` gain **no**
  `serde::Serialize` derive and **no** inert-violating field (only id / status / result /
  depth / opaque detail).
- The golden report **clearly reports which checks ran vs skipped** (spec §14 note) as a
  machine-readable artifact, with `conformant: true` on the valid fixture.
- A mutated golden (extra key / bad enum) is rejected by the schema.
- The report shape is versioned implicitly by `format_version` only (documented in the
  schema + `conformance/README.md`).
- Commit via the bump+tag convention.

**Spec refs.** §1 (inert report shape), §10 (validate = the spec executed, JSON-emitting),
§14 (every check id appears in the report), §14 note (the validator MUST clearly report
which checks ran); R4 (stable JSON shape); architecture §5, §7 R3/R4.

**Commit message.** `feat(core): pin validate.schema.json and golden validation report for the valid fixture`

---

## Coverage map — every MS6 deliverable / exit criterion / spec ref is assigned

| MS6 deliverable / exit criterion (milestones.md, as folded) | Step(s) |
|---|---|
| `validate(path) -> Result<ValidationReport, ValidateError>` in `hdx-core` | S1 (skeleton + entry gate + in-memory checks) → S2 (cross-file checks) |
| §0 hard version cut + manifest boundary-parse **FIRST** (M1/M2 entry gate, fail-closed) | S1 (entry gate, mirrors `describe`) |
| Manifest M3, M4 (six fields; created_at RFC3339; crs/cadence non-empty) | S1 (folded into the entry gate; in-memory negatives) |
| Manifest M5 (crs matches every georeferenced file) | S2 (`check_m5`, manifest CRS vs each `GridInfo`/outlines CRS) |
| Manifest M6 (cadence vs realized time axes) — **FOLD MED-1**: non-empty cadence (ran) + per-basin axis regularity (R3-skip in v0.1); no cross-basin equality, no cadence-word interpretation | S2 (`check_m6` + doc-comment limit) |
| Layout L1 (root rollups), L2 (basin dirs + artifacts derived from field set), L3 (no stray/ragged; absence=NaN) | S2 (`check_l1`/`check_l2`/`check_l3`) |
| Identity I1 (basin_id column present), I2 (in-file == folder), I3 (unique) | I1/I2 → S2; I3 → S1 (in-memory) |
| Homogeneity H1 (identical schema), H2 (identical grid-label set) | S1 (in-memory negatives) |
| Time T1 (time named/typed/non-null/sorted), T2 (intra-basin axis identity, gaps NaN) | T1 → S1; T2 → S2 |
| Grids G1 (self-naming, no channel axis), G2 (shared label ⇒ alignment), G3 (CF/GeoTIFF georef) | G1 → S1; G2/G3 → S2 |
| Geometry Geo1 (outlines schema/label, not partitioned) | S2 (`check_geo1` from MS4 read) |
| Every check records **ran vs skipped** + R3 depth class; report states which ran (§14 note) | S1 (the model) + S2 (per-check) + S3 (machine-readable golden) |
| Byte-deep / undecodable items reported `skipped` with a reason (M6 regularity; §8 sharding note) | S2 (honest R3 skips) + S3 (in the golden) |
| Companion-mask & `{source}_{variable}` validated as **ordinary** (no suffix/prefix special-casing) | S2 (assertion; H1/G1 apply no name magic) |
| `ValidationReport` JSON serialization (per-check outcomes + `conformant`) | S3 (DTO + `validate_json`) |
| Positive paths proven on the MS2 valid fixture (incl. G2 positive on the shared aligned label) ⇒ `conformant:true` | S2 (fixture test) + S3 (golden) |
| `conformant:false` on **both** MS2 invalids, each pinning its one check (M2/entry-gate; L1) | S2 (both invalids) |
| In-memory negative unit test for every in-memory-falsifiable check (≥ H1, H2, I3, M3, M4, T1, G1) — **FOLD MED-2** | S1 (mandatory per-check) + S2 (M5/I2 in-memory legs) |
| On-disk negative matrix (I2 folder, T2 axis, G2 misalign, G3 georef, L1/L2/L3, Geo1, M5 crs-mismatch) **deferred to MS8** | S2 (explicit deferral statement) |
| No `regrid`/`clip`/`reduce`; no inert-violating field; manifest exactly six; `format_version` hard cut; no gridded-chunk/pixel decode (LOW-3) | S1–S3 (scope guard, asserted by construction) |
| Every step: build + test + clippy `--all-targets -D warnings` + bump+tag | S1–S3 |

**Note on the report-vs-error split (recorded in S1).** A **violated MUST that ran** is a
`CheckOutcome { result: Fail }` ⇒ `conformant:false` — never a returned `Err`. A
**structural** failure (unreadable dataset dir, undecodable present artifact, unreadable
manifest) and the **§0 hard cut** (unknown `format_version`) are a returned
`ValidateError` — mirroring `describe`, so the CLI (MS7) can map them to exit code 2 while
mapping `conformant:false` to exit code 1. This split keeps `validate` fail-closed (a real
MUST violation is always observable as non-conformant) while keeping the §0 hard cut and
true IO faults distinct from a conformance verdict — exactly as the spec §0 entry
discipline requires.
