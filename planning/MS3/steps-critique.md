# MS3 STEP-PLAN CRITIQUE (adversarial review)

**Reviewer verdict:** APPROVED with low/medium issues (no high/critical). The plan is
in scope, fully covers the MS3 deliverables + exit criteria, is correctly ordered,
each step is independently green + committable, conventions are honored, and all three
mandated STEP-2 folds (MED-5, LOW-1, hidden/OS-cruft robustness) are genuinely
incorporated. The issues below are quality/precision tightenings, not blockers.

Reviewed against: `planning/MS3/steps.md`, `spec/HDX_SPEC.md`, `architecture.md`,
`planning/milestones.md` (MS3), and the actual repo state (MS1 types in
`crates/core/src/`, MS2 fixtures in `conformance/`).

---

## Ground-truth verification (repo facts the plan rests on)

Confirmed against the real tree (not just the plan's claims):

- **MS1 types exist and match plan assumptions.** `Field::new(name, quadrant, dtype,
  units, grid_label)` (`field.rs:246`), `parse_dtype(&str) -> Result<Dtype, CoreError>`
  (`field.rs:169`), `Quadrant::{ScalarStatic, ScalarDynamic, ...}`, `Dtype::{F64,
  Timestamp, ...}`, `BasinId::new` (`newtypes.rs:23`). `parse_dtype` accepts `"f64"`
  and `"timestamp"` — exactly the arrow→dtype targets S3 maps to. Good.
- **`CoreError` is `#[non_exhaustive]`** (`error.rs:17`) and the crate contains **no
  exhaustive `match` on it** (only constructions in `field.rs`/`manifest.rs`/
  `format_version.rs` and a construction list in the `lib.rs` test). => Adding the new
  `ParquetRead`/`LayoutWalk`/`MissingScalarColumn` variants in S1/S2/S3 **does not break
  the build** and leaves the tree green. Confirmed safe.
- **MS2 fixtures match the plan's concrete test expectations exactly:**
  - `scalar_static.parquet`: `basin_id: string not null`, `drainage_area: double` →
    S3's "`ScalarStatic` field `drainage_area` (f64)" is correct.
  - `basin=0001/scalar_dynamic.parquet`: `basin_id`, `time: timestamp[us] not null`,
    `streamflow: double` → S3's `ScalarDynamic`/`streamflow`/f64 + non-null sorted
    `time` is correct.
  - **Row-group `time` statistics are present and Rust-recoverable-shaped** (`has_min_max
    = True`, min=`2000-01-01`, max=`2000-01-05` for basin=0001) → the MED-5 statistics
    path is genuinely live; S3's `source == Statistics` + `start=2000-01-01,
    end=2000-01-05` assertions match the bytes.
  - Per-basin row counts 0001=5, 0002=7, 0003=4 → S3/S4's ragged-extent assertion is
    correct.
- **There is NO `*_was_filled` scalar column on the fixture** (the only data columns are
  `drainage_area` / `streamflow`; the `_was_filled` companion lives in the Zarr). The
  plan handles this correctly: it proves no-name-magic against an **in-test** synthesized
  `streamflow_was_filled` scalar column rather than expecting it in the fixture (S3 test
  plan). Scope-honest.
- **`bump-version.sh` edits the workspace-root `Cargo.toml`**; the per-step "stage
  `Cargo.toml` + bump + tag" is consistent with that. Tags currently at `v0.1.9`.

---

## Scope (no creep, no later-milestone work, inert/agnostic honored)

PASS. Every step stays inside MS3 (milestones.md MS3 / architecture §1):

- Metadata + 1-D `time`-column reads only; no gridded chunk decode; no Zarr/COG/
  geoparquet *schema* parse (S2 records `outlines.geoparquet` + gridded subtrees as
  presence/paths only, deferred to MS4). Verified the gridded subtree is recorded as a
  *path*, not parsed.
- No `describe`/`validate`/§14 rule engine/CLI/PyO3; no regrid/clip/reduce; no fixture
  mutation.
- Inert/agnostic: the scalar field catalog reuses MS1 `Field` (the five inert fields)
  unchanged; no transform/role/semantic/provenance/reduction field is introduced. The
  six-field `Manifest` is untouched. `TimeExtentSource` (`Statistics` vs
  `BoundedColumnScan`) is **read-provenance** (which read path produced the extent), NOT
  data/computation provenance — it does not record "what was done to the data," so it
  does not violate spec §1. (Called out because a careless reviewer could mistake the
  word "provenance" for the forbidden kind; it is not.)
- The one architecture edit (S1, R1-parquet amendment in the Amendments log) is
  explicitly invited by the architecture header and does not contradict the spec.

`basin_id` folder-vs-in-file is **recorded as a pair**, not adjudicated (I2 deferred to
MS6). Correct.

---

## Coverage (every MS3 deliverable + exit criterion + spec ref assigned)

PASS. Cross-checked the plan's coverage table against milestones.md MS3 deliverables and
exit criteria — no gap:

| MS3 deliverable / exit criterion | Covered by | OK |
|---|---|---|
| `parquet`/`arrow` deps + R1-parquet decision recorded | S1 | ✓ |
| Layout-walk → typed layout model (root + per-basin paths) | S2 | ✓ |
| "absent gridded subtree" vs "missing required file" distinction (L1/L2/L3 foundations) | S2 (facts) + S4 (gaps-as-facts) | ✓ |
| Hidden/dot/OS-cruft ignored (pre-empt MS6 L3 false positive) | S2 | ✓ |
| Scalar reader: arrow schema → `Vec<Field>` (quadrants, dtypes via `parse_dtype`) | S3 | ✓ |
| `basin_id` presence/value | S3 + S4 (paired) | ✓ |
| `time` descriptor (name/type/nullability/sort) | S3 | ✓ |
| Per-basin time extent from row-group statistics + bounded 1-D fallback | S3 | ✓ |
| Fallback classified under R3 + reader docs (LOW-1) | S3 + S5 | ✓ |
| MED-5: fixture `time` carries Rust-readable stats AND extent comes from stats (not fallback) | S3 | ✓ |
| MED-5 mismatch ⇒ regenerate fixture, never reader workaround (in docs) | S3 + S5 | ✓ |
| Typed errors (named-field thiserror) for malformed parquet / missing columns | S1/S2/S3 | ✓ |
| Tests over `conformance/valid/minimal/` (basins, schema, basin_id, time) | S2/S3/S4 | ✓ |
| Companion-mask / `{source}_{variable}` scalar fields catalogued ordinary | S3 (in-test) + S4 | ✓ |
| Scalar half of shared discovery layer (one boundary fn) | S4 | ✓ |
| Spec MUST advanced: L1/L2/L3, I1, T1 | S2 (L1/L2/L3) + S3 (I1,T1) + S4 | ✓ |
| Crate README / module docs (Mermaid + glossary) | S5 | ✓ |
| build+test+clippy green after every step; bump+tag | S1–S5 | ✓ |

No MS3 deliverable is left unassigned; no step claims a deliverable it cannot deliver.

---

## Ordering (buildable as written, no forward dependency)

PASS. S1 (deps + private metadata helper + error variant) → S2 (filesystem-only layout
walk, needs only MS1 newtypes) → S3 (reader, needs S1's parquet helper + S2's paths) →
S4 (assembles S2+S3 into one boundary fn) → S5 (docs reflecting final shape). No step
depends on a later one. Each compiles on its own.

---

## Green / committable (each step independently green; no bundled changes)

PASS, with one precision note (LOW-3 below). Each step adds its own module + tests and
keeps build/test/clippy green:

- S1's new error variant + module is exercised by an in-test parquet buffer; no
  exhaustive `CoreError` match exists to break (verified).
- S2/S3/S4 each land green with tests against the real MS2 fixtures.
- Adding variants to the `#[non_exhaustive]` `CoreError` cannot turn the tree red.

---

## Conventions (no baked-in violations)

PASS. The plan explicitly mandates: `tracing` (no `println!`), `#[instrument]` on public
fns, named-field `thiserror` variants each doc-commented with *when* they fire, typed
errors (no `unwrap`/`expect`/panic in lib), enums over booleans (`TimeExtentSource`,
`Quadrant`), parse-at-boundary (arrow types → MS1 `Dtype` via `parse_dtype`; folder name
→ `BasinId`). No `bool` for a domain state. No manifest extra/missing-field handling
changes. No `use super::*` implied (MS1 uses explicit imports; plan should keep that —
see LOW-2).

---

## Folded STEP-2 issues — verification

1. **MED-5 hand-off (Rust-side confirmation) — GENUINELY FOLDED.** S3 has a *dedicated*
   test that (a) opens `basin=0001/scalar_dynamic.parquet` with the Rust `parquet` crate
   and asserts the `time` column's row-group stats expose usable `min`/`max`, and (b)
   asserts `time_extent(...)` returns `source == TimeExtentSource::Statistics` (not the
   fallback) with the exact fixture extent. The "mismatch ⇒ regenerate MS2 fixture, never
   a reader workaround" rule is stated in the reader module docs. Verified the fixture
   actually carries `has_min_max=True` stats, so this is testable, not aspirational. Both
   halves (stats readable + extent-from-stats) asserted. ✓

2. **LOW-1 (bound the fallback) — GENUINELY FOLDED.** The plan dedicates a whole section
   ("The LOW-1 bounded-fallback discipline") and S3 implements it: fallback projects
   **only the `time` column by name** (never a data column, never a chunk), is classified
   under **R3** as a metadata/index-tier, architecture-§1-compliant 1-D read (NOT a
   gridded-chunk decode), the bound is stated in function + module docs, and provenance
   is recorded (`BoundedColumnScan`). A test exercises it against an **in-test**
   statistics-stripped asset and asserts the data column is never read. ✓

3. **Robustness (hidden/OS cruft) — GENUINELY FOLDED.** S2 adds a pure
   `is_ignored_entry(&str) -> bool` predicate (dotfiles/`.DS_Store`/`.gitkeep`/`.git`/…)
   so the walk never enumerates cruft as an HDX path or basin dir, with a test seeding
   `.DS_Store`/`.gitkeep`/dotfiles at the root and inside a basin dir and asserting only
   HDX paths are discovered. Explicitly framed as pre-empting an MS6 L3 false positive. ✓

None of the three folds is cosmetic.

---

## ISSUES

### MED-1 — S2 walk must explicitly NOT recurse into `gridded_*` subtrees / Zarr stores (stray-file false-positive risk)
- **Severity:** medium · **Category:** missing-coverage / convention
- The real `gridded_dynamic/era5.zarr` store contains many internal entries
  (`zarr.json`, `c/0/0/0` chunk dirs, per-variable subdirs) and `gridded_static/era5.tif`
  is a file. S2 says it records the gridded subtrees "as paths only … not read here,"
  which is correct, but the **acceptance criteria and robustness test do not pin that the
  walk stops at the `gridded_static/`/`gridded_dynamic/` boundary** (records their
  presence/path without recursing into Zarr internals). If a future implementer makes
  `walk_layout` recurse generically, the Zarr `c/`/`zarr.json` entries (none are dot-
  prefixed, so `is_ignored_entry` will not filter them) could be enumerated as HDX paths
  or counted as stray files — the exact L3 false positive the robustness fold is meant to
  prevent, but for the non-dot case.
- **Suggested fix:** Add an explicit S2 acceptance bullet + test: `walk_layout` records
  the `gridded_static/`/`gridded_dynamic/` subtree as a single recorded path/presence
  fact and **does not descend** into it (assert the Zarr `zarr.json`/`c/` entries are
  never enumerated). State the walk's recursion bound (root level + one level into each
  `basin=<id>/`, treating `gridded_*` as opaque leaves for MS3) in the `layout.rs` docs.

### LOW-1 — lib.rs `variants.len() == 13` error-surface test intent drifts as variants are added
- **Severity:** low · **Category:** convention / not-green-adjacent
- `crates/core/src/lib.rs:79` has `every_core_error_variant_constructs` asserting
  `variants.len() == 13` over a hand-built list of "every" variant. Adding `ParquetRead`
  (S1), `LayoutWalk` (S2), `MissingScalarColumn` (S3) does **not** break this test (it is
  a hardcoded list, so it stays green), but it silently violates the test's documented
  intent ("Constructs every CoreError variant so the error surface is exercised"): the
  new variants won't be exercised there and the count becomes stale. No plan step
  mentions this test.
- **Suggested fix:** In S1/S2/S3, when adding each variant, extend the `lib.rs` variant
  list and bump the asserted count (13→14→15→16), keeping the "every variant" invariant
  true. Add a one-line note to each step's Changes list. (Trivial, but currently
  unstated, so an implementer may leave the count stale.)

### LOW-2 — plan does not restate the "no `use super::*`" / explicit-import + grouped-import convention for the new modules
- **Severity:** low · **Category:** convention
- CLAUDE.md forbids `use super::*` and requires grouped imports (std → external →
  crate-internal). The MS1 modules honor this. The MS3 plan describes new modules
  (`layout.rs`, `scalar_reader.rs`, `discovery.rs`, `parquet_meta.rs`) but never restates
  this import discipline, and these modules will pull in `arrow`/`parquet` externals next
  to `crate::` imports.
- **Suggested fix:** Add a one-line convention reminder to the scope guard / each step:
  explicit imports only (no `use super::*`), grouped std → `arrow`/`parquet`/`tracing` →
  `crate::…`. Cheap insurance against a baked-in violation.

### LOW-3 — LOW-1 fallback test depends on the Rust `parquet` writer being able to disable row-group statistics; feasibility unstated
- **Severity:** low · **Category:** vague-acceptance / not-green-risk
- S3's fallback test builds an in-test parquet "with row-group statistics **disabled**
  (parquet writer option)" to force the `BoundedColumnScan` path. This is the correct
  approach, but the plan asserts the writer option exists without pinning it; if the
  pinned `parquet` writer cannot disable `time` statistics, S3's fallback test (and thus
  the step's green gate) is at risk.
- **Suggested fix:** Name the concrete mechanism in S3 (e.g. `WriterProperties` with
  statistics disabled / `set_statistics_enabled(EnabledStatistics::None)`), or state the
  fallback: if statistics cannot be disabled on write, synthesize the asset another way
  (e.g. write without the column-level stats path) — and confirm it during S1's crate
  smoke so the capability is proven before S3 relies on it.

### LOW-4 — S4 "compile-level seam test" / doc-test acceptance is slightly vague
- **Severity:** low · **Category:** vague-acceptance
- S4's "Seam check (review + a compile-level test)" and "a doc-test / comment notes where
  MS4's `GridInfo` + `delineations` attach" mixes a reviewer judgment with an
  unspecified test artifact. "Clean seam documented" is not a concrete, checkable
  criterion.
- **Suggested fix:** Make it concrete: assert the per-basin gridded-subtree *paths* are
  reachable from `ScalarDiscovery`/`LayoutModel` via a real test (so MS4 can attach
  without reshaping), and replace "clean seam" with the specific structural assertion
  (e.g. "`ScalarDiscovery` exposes `basins` + each `BasinDir`'s gridded subtree paths;
  a test reads them"). Keep the doc note, but back the acceptance with the assertion.

---

## ACCEPTANCE QUALITY

Acceptance criteria are mostly concrete: every step gates on `cargo build` + `cargo test`
+ `cargo clippy --all-targets -- -D warnings`, names specific spec-check ids advanced
(L1/L2/L3, I1, T1), and pins concrete fixture facts (3 basins `0001/0002/0003`; extents
`2000-01-01..2000-01-05`; row counts 5/7/4; `source == Statistics`). Commit messages are
conventional (`feat(core): …`, `docs(core): …`). The bump+tag discipline is stated per
step. The only soft spots are LOW-3 (writer-option feasibility) and LOW-4 (seam test).

---

## VERDICT

**APPROVED.** Zero high/critical issues; full coverage; correct ordering; each step
independently green and in scope; conventions honored; all three mandated STEP-2 folds
(MED-5, LOW-1, hidden/OS-cruft) genuinely incorporated and matched to real fixture bytes.
The five issues (1 medium, 4 low) are precision/robustness tightenings — chiefly MED-1
(pin that the layout walk treats `gridded_*` subtrees as opaque leaves and never
enumerates Zarr internals as stray files) — and should be folded into the steps, but
none blocks the plan.
