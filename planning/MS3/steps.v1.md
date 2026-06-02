# MS3 — Layout discovery + scalar parquet reader (discovery layer, scalar half) — STEP PLAN

> **Milestone:** MS3 (the third milestone of HDX v0.1; depends on MS1 types + MS2 fixtures).
> **Source contract:** `spec/HDX_SPEC.md` (canonical, settled).
> **Planned against:** `architecture.md` §1 (read metadata, not chunks), §3 (the type
> model / discovery types), §6 (sequencing hint), and `planning/milestones.md` (MS3
> goal, deliverables, reviewable outcome, exit criteria, spec refs, risks).
> **Folded STEP-2 critique (mandatory):**
> - **MED-5 hand-off (Rust-side confirmation).** MS3 confirms **from the Rust side**
>   that the MS2 valid fixture's parquet `time` column carries usable row-group
>   min/max statistics, and that the per-basin time extent is read **from those
>   statistics** (not the bounded-scan fallback) on that fixture — a dedicated test
>   asserts both. If the Rust `parquet` crate cannot surface the statistics pyarrow
>   wrote, the fix is to **regenerate the fixture** (an MS2 generator change), never
>   a reader workaround.
> - **LOW-1 (bound the fallback).** The time-extent bounded fallback used when
>   row-group statistics are absent reads **only** the `time` column (never any data
>   column), and is documented as **metadata/index-tier and architecture-§1-compliant
>   (NOT a gridded-chunk decode)**, so a reviewer cannot mistake it for a chunk read.
>   The bound is stated explicitly in the reader docs.
> - **Robustness (hidden/OS cruft).** The layout walk **ignores hidden/dot entries and
>   OS artifacts** (`.DS_Store`, dotfiles, `.gitkeep`, etc.) so such files in a working
>   tree are never enumerated as HDX paths or counted as stray files — only
>   HDX-relevant paths are discovered. This pre-empts an L3 stray-file false positive
>   in MS6.
>
> **Why this milestone is MS3.** MS1 (the Rust type model + manifest parser + manifest
> JSON Schema) and MS2 (the dev-only Python fixture generator: one valid four-quadrant
> dataset + two minimal invalids under `conformance/`) are built and green. MS3 is the
> next milestone: it builds the **scalar half of the shared discovery layer** that both
> verbs (`describe` MS5, `validate` MS6) will stand on — walking the basin-first hive
> tree, enumerating basins, and reading parquet **metadata** (schema → `Field`s,
> `basin_id` column, `time` column type/sort/nullability, per-basin time extent from
> row-group statistics). The gridded + geometry half is MS4.

---

## Scope guard

Every step below stays strictly inside MS3 (milestones.md MS3 / architecture §1):

- **Metadata + 1-D column reads only — never gridded chunks (architecture §1).** MS3
  reads parquet **schema/metadata** and, where it must touch data, only the **1-D
  `time` column** (and only as a bounded fallback). No parquet *data* column is ever
  scanned for values; no Zarr/COG/geoparquet bytes are read at all (those are MS4).
  This is the discovery-layer discipline: the structure and shape of data, never its
  scientific values.
- **Discovery only, no enforcement.** MS3 *reads and models* the layout + scalar
  schema; it **enforces no spec §14 check**. It produces the typed in-memory model
  and surfaces facts (root rollups present-or-absent, basin list, scalar field
  catalog, `basin_id` presence, `time` descriptor, per-basin time extents). The
  conformance verdict (L1–L3, I1, T1, …) is **MS6**, which runs rules over this model.
  Where MS3 distinguishes "absent because no gridded fields" from "missing required
  file," it records the *fact*; it does not *fail* on it.
- **Scalar half only.** MS3 reads `scalar_static.parquet` (root rollup) and each
  `basin=<id>/scalar_dynamic.parquet`. It confirms the **presence** of the other root
  rollup (`outlines.geoparquet`) as a layout fact, but does **not** parse its schema —
  that is MS4 (geometry reader). It reads **no** Zarr/COG metadata — that is MS4.
- **No later-milestone work.** No `describe` (MS5), no `validate` / §14 rule engine
  (MS6), no CLI (MS7), no PyO3 (MS9). No regrid/clip/reduce/reduction/hydrology
  anywhere (excluded forever, spec §10). No fixture generation or mutation (MS2/MS8).
- **Inert/agnostic discipline (hard rule, spec §1/§13).** No type or field introduced
  in MS3 carries transform / normalization / role / semantic-type / reduction /
  provenance / computation-source. The scalar field catalog reuses MS1's `Field`
  (exactly `name`, `quadrant`, `dtype`, `units`, `grid_label`) — nothing derivable is
  added. Scalar fields are catalogued **purely by physical schema**: the reader applies
  **no name-pattern special-casing** (no `_was_filled` suffix magic, no
  `{source}_{variable}` prefix split) — field names stay opaque producer strings
  (spec §2). The discovery model adds **no manifest-floor field**: the six-field
  `Manifest` is untouched.
- **`basin=<id>` is locality, `basin_id` is authority (spec §3).** MS3 *reads* both —
  the folder id (parsed from the partition directory name) and the in-file `basin_id`
  column — and records them side by side so MS6 can cross-check them (I2). MS3 does
  **not** itself decide agreement; it surfaces the pair.

No step performs a later milestone's work, and none violates the inert/agnostic
discipline.

---

## The MED-5 writer/reader hand-off (received from MS2, discharged here for parquet)

MS2 self-asserted (Python/pyarrow side) that the valid fixture's `time` column carries
usable row-group min/max statistics, and named MS3 as the **Rust-side confirmation**.
MS3 discharges that hand-off for the parquet half:

1. **Rust-side statistics confirmation.** A dedicated test opens
   `conformance/valid/minimal/basin=<id>/scalar_dynamic.parquet` with the Rust
   `parquet` crate, reads the `time` column chunk's row-group statistics, and asserts
   they expose usable `min`/`max`. (MS2 already verified pyarrow *wrote* them; this
   verifies Rust can *recover* them.)
2. **Extent-from-statistics confirmation.** The same/sibling test asserts the per-basin
   time extent the reader returns is sourced **from those statistics**, not from the
   bounded-scan fallback (the reader reports its provenance — `Statistics` vs
   `BoundedColumnScan` — and the test pins it to `Statistics` on this fixture).
3. **The hand-off rule (folds MED-5).** If the Rust `parquet` crate **cannot** surface
   the statistics pyarrow wrote, the correct fix is to **regenerate the MS2 fixture**
   (adjust the generator and re-emit), **never** a reader workaround that papers over a
   writer/reader mismatch. This is stated in the reader's module docs as a named MS2
   hand-off, so a future agent treats a mismatch as a generator bug.

The MS2 → MS4 hand-off (Zarr consolidated metadata) is **out of MS3 scope** and is
discharged in MS4.

---

## The LOW-1 bounded-fallback discipline (stated up front, implemented in S3)

When row-group statistics are **absent** for `time`, the reader falls back to a
**bounded 1-D `time`-column read** to recover `[min, max]`. This fallback is held to a
hard bound, documented in the reader's module + function docs:

- It reads **only the `time` column** — never any data column (`streamflow`,
  `drainage_area`, …) and never a gridded chunk. It selects the single `time` column by
  name via parquet column projection.
- It is classified explicitly under **R3** as a **metadata/index-tier read** that is
  **architecture-§1-compliant**: a 1-D coordinate/key-column read (the same tier as
  reading a Zarr `time` coordinate array), **NOT** a gridded-chunk decode. The docs
  state this so a reviewer cannot mistake the fallback for a values read.
- Its **provenance is recorded** on the returned extent (`TimeExtentSource::Statistics`
  vs `TimeExtentSource::BoundedColumnScan`) so downstream (MS5 `describe`, MS6 `validate`
  R3 classification) can report which path produced each extent.

On the MS2 valid fixture the **statistics** path is live; the fallback is exercised in
tests against a **locally-synthesized** statistics-stripped parquet (a test asset built
in-test, not a committed fixture), so both paths are proven without an MS2/MS8 fixture.

---

## Ordering rationale

The steps follow build-tractability and strict dependency order; each is one
conventional commit and leaves the repo **green** (`cargo build` + `cargo test` +
`cargo clippy --all-targets -- -D warnings` all pass after the step):

1. **S1 — add the `parquet`/`arrow` dependency + record the R1-parquet decision; a
   trivially-exercised crate smoke surface.** Everything downstream needs the
   pure-Rust `arrow`/`parquet` stack on `crates/core`. Doing the dependency add +
   architecture-amendment (R1 for parquet) **first**, with a minimal compiling+tested
   touchpoint (a private helper that opens a parquet file's metadata and a unit test
   over a tiny in-test parquet buffer), de-risks the crate choice and keeps the diff
   reviewable before any module structure exists. Green and committable on its own.
2. **S2 — the layout-walk module (typed layout model).** Needs only MS1 newtypes +
   S1's nothing-in-particular (it is filesystem-only). Walks the basin-first hive:
   discovers root rollups, enumerates `basin=<id>` dirs (parsing the folder id),
   collects per-basin artifact paths, and **ignores hidden/dot/OS-cruft entries**
   (folds the robustness item). Produces the typed `LayoutModel`. No parquet reads yet
   — pure directory structure — so it is independently testable against the MS2 trees
   and the two invalids, and is the spine S3/S4 hang scalar facts on.
3. **S3 — the scalar-parquet reader (schema → fields, `basin_id`, `time` descriptor,
   per-basin time extent).** Needs S1 (the parquet crate) and S2 (the layout model with
   the parquet paths). Reads the arrow schema into MS1 `Field`s (scalar quadrants,
   dtype via MS1 `parse_dtype`), reads the `basin_id` column presence/value, reads the
   `time` column descriptor (name/logical-type/nullability/sort), and computes the
   per-basin time extent **from row-group statistics with the LOW-1 bounded fallback**.
   Discharges the **MED-5** Rust-side statistics confirmation against the MS2 fixture.
4. **S4 — assemble the scalar half of the shared discovery model + wire it through the
   layout walk into one typed entry point.** Needs S2 + S3. Introduces the
   discovery-layer types this milestone owns (the scalar half: basin list, scalar field
   catalog, per-basin `time` descriptors + extents, `basin_id` observations,
   root-rollup presence) and a single `discover_scalar(path) -> Result<ScalarDiscovery,
   …>` boundary function that walks + reads + returns the typed model. Proves the whole
   scalar half populates from the real MS2 fixture in one call. Asserts scalar fields
   are catalogued as **ordinary** (no name-pattern special-casing). Leaves clean seams
   for MS4 to attach the gridded/geometry half.
5. **S5 — crate-README + module docs update (Mermaid map + glossary) for the discovery
   layer (scalar half).** Pure docs: extend `crates/core/README.md`'s Mermaid module
   map + glossary with the new layout/scalar-reader/discovery modules and the domain
   terms (layout model, root rollup, time extent + its provenance, the LOW-1 bounded
   fallback, the MED-5 hand-off). Placed last so the docs reflect the final MS3 shape.

Each step is one conventional commit, ends with `./scripts/bump-version.sh patch` +
stage `Cargo.toml` + commit + `git tag v<version>` (CLAUDE.md / architecture §2), and
after it `cargo build` + `cargo test` + `cargo clippy --all-targets -- -D warnings`
all pass.

---

## Steps

### MS3-S1 — Add the pure-Rust `arrow`/`parquet` stack; record R1-parquet decision

**Intent.** Add the format-reader dependency MS3 stands on — the mature pure-Rust
`arrow`/`parquet` stack (no GDAL) — and record the **R1 decision for parquet** as an
architecture amendment, with a minimal, fully-tested crate touchpoint so the choice is
exercised (not just declared) and the diff stays reviewable before any module is built.
Independently committable: it adds a dependency, one small private helper, and a unit
test over a tiny in-test parquet buffer; it changes no public API and leaves the crate
green.

**Changes.**
- `crates/core/Cargo.toml` — add `parquet` and `arrow` (pinned to a compatible
  major; document the exact versions in the amendment). `arrow` is needed for the
  arrow schema / array types `parquet` surfaces; keep features minimal (no async, no
  object-store) — local-filesystem metadata + column reads only.
- `architecture.md` **Amendments log** — a dated entry recording the **R1-parquet
  decision**: pure-Rust `arrow`/`parquet` chosen (mature, no GDAL system dep), the
  pinned versions, and the note that the Zarr/COG/geoparquet R1 decision is deferred to
  MS4. *(This is the one architecture edit MS3 makes; the architecture header invites
  amendments and forbids contradicting the spec — this entry does neither.)*
- `crates/core/src/parquet_meta.rs` (or a small private module, e.g. inside a new
  `reader/` dir) — a minimal private helper that opens a parquet byte source and
  returns its file metadata (arrow schema + row-group metadata). Used only by S3's
  reader later; S1 lands it with a unit test so the crate exercises the dependency now.
  `#[instrument]` on any public surface; `tracing` (`debug`) for diagnostics; typed
  errors via a new `CoreError` variant (see below) — no `unwrap`/`expect`/panic.
- `crates/core/src/error.rs` — add a named-field `thiserror` variant
  `ParquetRead { artifact: String, source-detail }` (e.g. `ParquetRead { artifact:
  String, detail: String }`) doc-commented with *when* it fires (a parquet file fails
  to open or its metadata fails to decode), so the reader has a typed error surface.
  *(MS1 reserved several skeleton variants; this is a genuinely new one MS3 needs.)*
- `crates/core/src/lib.rs` — declare the new module(s).

**Test plan.**
- A unit test builds a tiny parquet buffer **in-test** (using the `parquet` crate's
  writer, dev-only path — write a 1-column `int32` table to an in-memory `Vec<u8>`),
  then calls the helper and asserts it recovers the arrow schema (column name + type)
  and the row-group count. This exercises the pinned crate end-to-end.
- A negative test: feeding non-parquet bytes returns `CoreError::ParquetRead`
  (typed error, never a panic).
- **Gate:** `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings`
  all pass.

**Acceptance.**
- `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings` all pass.
- `parquet`/`arrow` added to `crates/core/Cargo.toml`; the **R1-parquet decision is
  recorded** as a dated `architecture.md` amendment (pure-Rust, no GDAL, pinned
  versions; Zarr/COG/geoparquet deferred to MS4).
- The crate compiles **and a test exercises** the parquet metadata path (schema +
  row-group recovery) over an in-test buffer; malformed input returns
  `CoreError::ParquetRead` with no panic (no `unwrap`/`expect` in library code).
- Spec MUST-checks advanced: foundation for **T1**/**I1** reads (the metadata path the
  scalar reader will use); architecture §1 honored (metadata read, no chunk decode).
- Inert/agnostic intact: no new domain field; the new error variant carries only an
  artifact name + a detail string.
- Patch-bump + stage `Cargo.toml` + conventional commit + `git tag v<version>`.

**Spec refs.** §4 (parquet is the scalar physical encoding), §8 (parquet metadata /
row-group statistics are the read target), §14 T1/I1 (the metadata the reader feeds);
architecture §1 (metadata reads, not chunks), §7 R1 (reader-crate selection — parquet
half decided here).

**Commit message.** `feat(core): add pure-Rust arrow/parquet reader stack (R1)`

---

### MS3-S2 — Layout-walk module: typed `LayoutModel` over the basin-first hive

**Intent.** Build the basin-first hive walk that turns a dataset directory into a typed
in-memory **`LayoutModel`**: the root rollups (present/absent), the enumerated
`basin=<id>` directories (with the folder id parsed out), and each basin's artifact
paths (`scalar_dynamic.parquet`, and the `gridded_static/` / `gridded_dynamic/`
subtrees recorded as *paths only*, parsed in MS4). The walk **ignores hidden/dot
entries and OS artifacts** so working-tree cruft is never enumerated as an HDX path
(folds the robustness item). Filesystem-only — no parquet reads — so it is independently
testable against the three MS2 trees and is the spine S3/S4 hang scalar facts on.

**Changes.**
- `crates/core/src/layout.rs` (new module) — the typed layout model and the walk:
  - `LayoutModel` — root rollup presence (`scalar_static.parquet`,
    `outlines.geoparquet`: each `present: bool` + its resolved path), and a
    `Vec<BasinDir>` of discovered basins.
  - `BasinDir` — the parsed folder id (`BasinId` from the `basin=<id>` directory name),
    the directory path, the `scalar_dynamic.parquet` path (present/absent), and the
    `gridded_static/` / `gridded_dynamic/` subtree presence + paths (recorded for MS4;
    **not** read here). It records facts; it does **not** decide L2.
  - A pure helper `parse_basin_dir_name(&str) -> Option<BasinId>` that recognizes the
    `basin=<id>` pattern and extracts `<id>` (returns `None` for non-matching names so
    the walk skips them).
  - **Hidden/OS-cruft filter (folds robustness).** A pure predicate
    `is_ignored_entry(&str) -> bool` that returns `true` for any entry whose name
    starts with `.` (dotfiles/dirs: `.DS_Store`, `.gitkeep`, `.git`, `.ipynb_checkpoints`)
    and any other documented OS artifact, so the walk **never** enumerates such entries
    as HDX paths or basin dirs. Documented as pre-empting an L3 stray-file false
    positive in MS6.
  - `walk_layout(path) -> Result<LayoutModel, CoreError>` — `#[instrument]`, reads the
    root directory (skipping ignored entries), records the two rollups' presence,
    enumerates `basin=<id>` dirs via `parse_basin_dir_name`, and for each records its
    artifact paths. Typed errors (a new named-field `CoreError` variant, e.g.
    `LayoutWalk { path: String, detail: String }`, doc-commented) for an unreadable
    directory — no `unwrap`/`expect`/panic. `tracing` (`debug`/`info`) for the basins
    discovered.
- `crates/core/src/error.rs` — add the `LayoutWalk { .. }` named-field variant,
  doc-commented with *when* it fires (the dataset path is not a readable directory).
- `crates/core/src/lib.rs` — declare `pub mod layout;`.

**Test plan.**
- Walk `conformance/valid/minimal/`: assert both root rollups present; exactly **3**
  basins enumerated (`0001`, `0002`, `0003`); each `BasinDir` has its
  `scalar_dynamic.parquet` path present and its `gridded_static/`/`gridded_dynamic/`
  subtree paths recorded.
- Walk `conformance/invalid/missing-root-rollup/`: assert the *fact* that one root
  rollup is **absent** is recorded (the walk does **not** fail — L1 enforcement is
  MS6); basins still enumerate.
- Walk `conformance/invalid/wrong-format-version/`: assert it walks identically to the
  valid tree (the mutation is only in `manifest.json`, which the walk does not read).
- **Robustness test (folds the cruft item):** create a temp dataset dir (copy or
  synthesize a tiny tree) seeded with `.DS_Store`, a `.gitkeep`, and a stray dotfile at
  the root and inside a basin dir; assert `walk_layout` enumerates **only** the HDX
  paths (the cruft is neither a basin dir nor a recorded artifact, and is not counted as
  a stray file). Unit tests on `parse_basin_dir_name` (accepts `basin=01013500`, rejects
  `basinx`, `basin`, `.DS_Store`) and `is_ignored_entry`.
- Negative: `walk_layout` on a non-existent / non-directory path returns
  `CoreError::LayoutWalk` (typed, no panic).
- **Gate:** `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings`
  pass.

**Acceptance.**
- `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings` all pass.
- `walk_layout` produces a typed `LayoutModel` over the real MS2 trees: root-rollup
  presence recorded; basins enumerated from `basin=<id>` dirs with the folder id parsed;
  per-basin artifact paths (incl. the gridded subtree *paths* for MS4) recorded.
- **Hidden/dot/OS-cruft entries are ignored** — a test proves `.DS_Store`/`.gitkeep`/
  dotfiles are never enumerated as HDX paths or basin dirs (pre-empts MS6 L3 false
  positive).
- The walk **records facts, enforces nothing**: the missing-root-rollup tree walks
  without error (L1 is MS6); the gridded-subtree-present vs absent distinction is
  recorded for L2's later use.
- Spec MUST-checks advanced (discovery foundations; enforced MS6): **L1** (root-rollup
  presence model), **L2** (basin-dir shape + per-basin artifact paths), **L3** (only
  HDX-relevant paths discovered; cruft ignored).
- Patch-bump + stage `Cargo.toml` + conventional commit + `git tag v<version>`.

**Spec refs.** §3 (`basin=<id>` locality; folder id), §4 (basin-first hive; root
rollups; per-basin artifacts), §5 (homogeneity — discovery is a per-basin model),
§14 L1–L3; architecture §1 (structure read), §5 (layout L1–L3 mapping).

**Commit message.** `feat(core): walk the basin-first hive into a typed LayoutModel`

---

### MS3-S3 — Scalar-parquet reader: fields, `basin_id`, `time` descriptor, time extent

**Intent.** Read parquet **metadata** for `scalar_static.parquet` and each
`scalar_dynamic.parquet` into typed facts: the scalar field catalog (arrow schema →
MS1 `Field`s for the scalar quadrants, dtypes via MS1 `parse_dtype`), the `basin_id`
column presence/value, the `time` column descriptor (name, logical type, nullability,
sort), and the **per-basin time extent computed from row-group statistics with the
LOW-1 bounded `time`-only fallback**. Discharges the **MED-5** Rust-side statistics
confirmation against the MS2 valid fixture. Independently committable: it adds the
reader + its tests over the real fixture (and an in-test statistics-stripped asset for
the fallback) and leaves the crate green.

**Changes.**
- `crates/core/src/scalar_reader.rs` (new module) — the scalar-parquet reader:
  - `read_scalar_static(path) -> Result<ScalarTable, CoreError>` and
    `read_scalar_dynamic(path) -> Result<ScalarDynamicTable, CoreError>` (names
    illustrative) that open the parquet metadata (via S1's helper) and extract:
    - the arrow schema mapped to a `Vec<Field>` (MS1 `Field::new`): each non-`basin_id`,
      non-`time` column → a `Field` with the right scalar `Quadrant`
      (`ScalarStatic` for the static rollup, `ScalarDynamic` for the per-basin dynamic
      table), `dtype` via MS1 `parse_dtype` over the arrow physical type, units `none`
      (parquet column metadata carries no units in the fixture — recorded as absent,
      not invented), and `grid_label: None` (scalar). Mapping arrow physical types →
      the canonical dtype strings MS1 accepts is a small documented table (e.g.
      `Float64` → `"f64"`, `Timestamp(..)` → `"timestamp"`); an unmapped arrow type
      returns `CoreError::UnknownDtype` (no panic).
    - `basin_id` column **presence** and, for the dynamic table, the **distinct
      in-file `basin_id` value(s)** (read from the column — a 1-D key-column read,
      architecture-§1-compliant), recorded for MS6's I2 folder cross-check. (MS3
      records the value; it does not compare to the folder — that is MS6.)
    - the `time` column **descriptor** for the dynamic table: a typed
      `TimeColumn { name, logical_type, nullable, sorted_ascending }`. `name` and
      `nullable` come from the arrow schema; `logical_type` is mapped to MS1 `Dtype`
      (`Timestamp`); `sorted_ascending` is determined from row-group statistics where
      present (min/max monotonic across row groups) or via the bounded `time`-only scan
      otherwise. MS3 **records** these facts; T1 enforcement is MS6.
  - **Per-basin time extent (the core MED-5/LOW-1 deliverable):**
    `time_extent(path) -> Result<TimeExtent, CoreError>` returning
    `TimeExtent { start, end, source: TimeExtentSource }` where
    `TimeExtentSource ∈ { Statistics, BoundedColumnScan }`:
    - **Statistics path (primary, §8).** Read the `time` column's row-group min/max
      statistics; the extent is `[min over row groups, max over row groups]`, with
      `source = Statistics`.
    - **Bounded fallback (LOW-1).** When statistics are **absent**, project **only the
      `time` column** (never any data column) and read it to recover `[min, max]`, with
      `source = BoundedColumnScan`. Documented in the function + module docs as a
      **metadata/index-tier, architecture-§1-compliant 1-D column read — NOT a
      gridded-chunk decode**, with the bound stated explicitly (one column, by name).
  - Typed errors via `CoreError` (reuse `ParquetRead`; add a named-field
    `MissingScalarColumn { artifact, column }` variant if a required column — e.g.
    `time` in a dynamic table — is absent at the schema level, doc-commented). No
    `unwrap`/`expect`/panic. `#[instrument]` on public fns; `tracing` for diagnostics.
- `crates/core/src/error.rs` — add `MissingScalarColumn { artifact: String, column:
  String }` (doc-commented: fires when the scalar reader cannot find a structurally
  required column).
- `crates/core/src/lib.rs` — declare `pub mod scalar_reader;`.

**Test plan.**
- **Field catalog (against `conformance/valid/minimal/`):**
  - `scalar_static.parquet` → one `ScalarStatic` field `drainage_area` (dtype `f64`),
    plus `basin_id` recognized as the id column (not catalogued as a data field).
  - `basin=0001/scalar_dynamic.parquet` → one `ScalarDynamic` field `streamflow`
    (dtype `f64`), `basin_id` recognized, `time` recognized as the time column.
- **`basin_id`:** present in `scalar_static` and in each `scalar_dynamic`; the in-file
  value for `basin=0001` is `"0001"` (recorded for MS6's I2 — not compared here).
- **`time` descriptor:** name `time`, logical type → `Dtype::Timestamp`, **non-nullable**
  (`timestamp[us] not null` in the fixture), `sorted_ascending == true`.
- **MED-5 Rust-side confirmation (dedicated test):** open
  `basin=0001/scalar_dynamic.parquet` with the Rust `parquet` crate, assert the `time`
  column's row-group statistics expose usable `min`/`max`, **and** assert
  `time_extent(...)` returns `source == TimeExtentSource::Statistics` (not the
  fallback) with `start = 2000-01-01T00:00:00`, `end = 2000-01-05T00:00:00` for that
  basin. A code comment + module doc state the MS2-regenerate hand-off rule (a missing
  Rust-readable statistic is a generator bug, fixed by regenerating MS2 — never a reader
  workaround).
- **Ragged-across-basins extents:** assert the three basins yield **different** extents
  (basin=0001: 5 rows, 0002: 7 rows, 0003: 4 rows per the fixture), confirming §6.1
  ragged extents are surfaced (read, not enforced).
- **LOW-1 bounded-fallback path:** build an **in-test** parquet asset for the `time`
  column with row-group statistics **disabled** (parquet writer option), assert
  `time_extent(...)` returns `source == TimeExtentSource::BoundedColumnScan` and the
  correct `[min, max]`, and assert (by construction/review) the fallback projects only
  the `time` column. A unit test confirms the fallback never reads a data column (e.g.
  the asset has a `data` column that, if read, would be observable — the test asserts
  the extent matches the `time` values regardless of `data`).
- **Ordinary-field discipline (scope-honest, folds the companion-mask intent for the
  scalar half):** the scalar fields on the MS2 fixture are `drainage_area`/`streamflow`
  — neither carries a special pattern. Assert the reader catalogs them **purely by
  physical schema with no name-pattern special-casing**; an additional unit test feeds
  an in-test scalar parquet whose data column is named `streamflow_was_filled` (a
  companion-mask-pattern name) and asserts it is catalogued as an **ordinary**
  `ScalarDynamic` field with **no suffix magic** (no role, no belongs-to). *(The
  on-disk companion-mask + `{source}_{variable}` fields live in the gridded Zarr and are
  asserted ordinary in MS4; MS3 proves the scalar reader applies no name magic.)*
- Negative: a dynamic table missing the `time` column → `CoreError::MissingScalarColumn`
  (typed, no panic); an unmapped arrow dtype → `CoreError::UnknownDtype`.
- **Gate:** `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings`
  pass.

**Acceptance.**
- `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings` all pass.
- The reader surfaces, from the real MS2 fixture: the scalar field catalog (quadrants +
  dtypes via MS1 `parse_dtype`), `basin_id` presence + in-file value, the `time` column
  descriptor (name/type/nullability/sort), and per-basin time extents.
- **MED-5 discharged (Rust side):** a test confirms the MS2 fixture's `time` column
  carries Rust-readable row-group statistics **and** the extent comes from
  `Statistics` (not the fallback); the reader docs name the MS2-regenerate hand-off
  rule (mismatch ⇒ regenerate fixture, never a reader workaround).
- **LOW-1 discharged:** the bounded fallback reads **only** the `time` column, records
  `source = BoundedColumnScan`, and is documented as an architecture-§1-compliant
  metadata/index-tier read (NOT a chunk decode), with the bound stated; both the
  statistics and fallback paths are tested.
- Scalar fields are catalogued as **ordinary** (no name-pattern special-casing),
  proven incl. a `_was_filled`-named in-test scalar column.
- Spec MUST-checks advanced (discovery foundations; enforced MS6): **I1** (`basin_id`
  column present in `scalar_static` + each `scalar_dynamic`), **T1** (`time`
  type/sort/non-null discovered), **L1** precondition (rollup readable); §6.1
  ragged extents surfaced.
- Patch-bump + stage `Cargo.toml` + conventional commit + `git tag v<version>`.

**Spec refs.** §2 (fields; ordinary; quadrant per field; no name magic), §3 (`basin_id`
column; authoritative in-file id), §6 / §6.1 (`time` column type/sort/non-null;
ragged-across extents), §8 (row-group statistics primary; sorted-by-`time`),
§14 I1, T1, L1; architecture §1 (metadata + 1-D column reads, not chunks), §3.5
(discovery types), §7 R3 (the fallback's metadata-vs-byte classification).

**Commit message.** `feat(core): read scalar parquet schema, basin_id, time + extents`

---

### MS3-S4 — Assemble the scalar half of the shared discovery model (one boundary fn)

**Intent.** Tie the layout walk (S2) and the scalar reader (S3) into the **scalar half
of the shared discovery layer**: a single typed model (`ScalarDiscovery`) and one
boundary function `discover_scalar(path)` that walks the tree, reads every scalar
artifact, and returns the typed in-memory model both verbs will later consume — the
basin list, the scalar field catalog, per-basin `time` descriptors + extents (with
their provenance), the in-file `basin_id` observations paired with their folder ids, and
the root-rollup presence facts. Proves the entire scalar half populates from the real
MS2 fixture in **one call**, and leaves clean seams for MS4 to attach the
gridded/geometry half. Independently committable: it adds the discovery types + the
boundary fn + an end-to-end test over the MS2 fixture; green.

**Changes.**
- `crates/core/src/discovery.rs` (new module) — the discovery-layer types this
  milestone owns (the **scalar half** of architecture §3.5's `Description` inputs;
  the gridded `GridInfo` + `delineations` are MS4):
  - `ScalarDiscovery` — `basins: Vec<BasinId>` (from the layout walk, sorted/stable),
    `scalar_fields: Vec<Field>` (the homogeneous scalar schema as discovered; MS3 reads
    one representative per quadrant from the rollup + a basin — H1 *enforcement* across
    basins is MS6), `per_basin: Vec<BasinScalar>`, and `root_rollups: RootRollupPresence`
    (the two rollups' present/absent facts from S2).
  - `BasinScalar` — `basin_id_folder: BasinId` (parsed from `basin=<id>`),
    `basin_id_in_file: Option<BasinId>` (read from the column; `None` if the column is
    absent), `time: Option<TimeColumn>`, `time_extent: Option<TimeExtent>` (with its
    `source`), and the basin's scalar field list. It records the **folder vs in-file id
    pair** side by side for MS6's I2 cross-check; MS3 does not decide agreement.
  - `discover_scalar(path) -> Result<ScalarDiscovery, CoreError>` — `#[instrument]`:
    calls `walk_layout` (S2), then for the root `scalar_static.parquet` and each basin's
    `scalar_dynamic.parquet` calls the S3 reader, assembling `ScalarDiscovery`. Surfaces
    discovery **gaps as facts** (absent rollup, absent column, fallback-sourced extent),
    never a verdict. Typed errors propagate from S2/S3.
- `crates/core/src/lib.rs` — declare `pub mod discovery;`; extend the module-map
  `//!` doc with the new modules.

**Test plan.**
- **End-to-end over `conformance/valid/minimal/`:** `discover_scalar(...)` returns a
  `ScalarDiscovery` with: 3 basins (`0001`/`0002`/`0003`); the scalar field catalog
  `{drainage_area: ScalarStatic/f64, streamflow: ScalarDynamic/f64}`; both root rollups
  present; for each basin a `BasinScalar` whose `basin_id_folder == basin_id_in_file`
  (recorded as a pair — asserted equal in the *test* to document the seam, **not**
  enforced as a rule), a non-nullable sorted `time` descriptor, and a `time_extent`
  with `source == Statistics`.
- **Ragged-across-basins (read, not enforced):** the three basins' extents differ
  (§6.1), surfaced as facts.
- **Gaps-as-facts over `conformance/invalid/missing-root-rollup/`:** `discover_scalar`
  **succeeds** (no verdict) and reports the absent rollup in `root_rollups` and the
  present basins — proving the discovery layer surfaces gaps without failing (L1
  enforcement deferred to MS6).
- **Ordinary fields:** assert the assembled `scalar_fields` carry no role/transform/
  semantic/provenance and no name-pattern special-casing (reusing S3's discipline at the
  assembled-model level).
- **Seam check (review + a compile-level test):** `ScalarDiscovery` exposes the data
  MS4 will extend (basins, per-basin gridded-subtree *paths* are already on the
  `LayoutModel`); a doc-test / comment notes where MS4's `GridInfo` + `delineations`
  attach.
- **Gate:** `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings`
  pass.

**Acceptance.**
- `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings` all pass.
- `discover_scalar(conformance/valid/minimal)` returns, in **one call**, the complete
  scalar half of the discovery model from real on-disk bytes: basin list, scalar field
  catalog (quadrants + dtypes), per-basin `time` descriptors + extents (with
  provenance), in-file-vs-folder `basin_id` pairs, and root-rollup presence facts.
- Discovery **surfaces gaps as facts, never a verdict**: the missing-root-rollup tree
  discovers successfully with the absent rollup recorded (L1 enforcement is MS6).
- The scalar half is structured so **MS4 attaches the gridded/geometry half** without
  reshaping it (clean seam documented).
- Spec MUST-checks advanced (discovery layer, scalar half complete; enforced MS6):
  **L1/L2/L3** (layout facts), **I1** (basin_id present), **I2** preconditions
  (folder-vs-in-file pair recorded), **I3** preconditions (in-file ids available),
  **T1** (time descriptor), **H1** preconditions (scalar schema discovered); §6.1
  ragged extents surfaced.
- Inert/agnostic: the model adds **no** manifest-floor field and **no** derivable/role/
  transform/semantic/provenance field; the six-field `Manifest` is untouched.
- Patch-bump + stage `Cargo.toml` + conventional commit + `git tag v<version>`.

**Spec refs.** §2 (fields; ordinary), §3 (`basin_id` authoritative + `basin=<id>`
locality pair), §4 (basin-first hive), §5 (one-basin discovery; homogeneity surfaced),
§6.1 (ragged time extents), §11 (six-field floor untouched — nothing derivable added),
§14 L1–L3, I1, I2/I3 preconditions, T1, H1 preconditions; architecture §1, §3.5
(discovery types), §5 (shared discovery layer both verbs consume).

**Commit message.** `feat(core): assemble the scalar half of the discovery layer`

---

### MS3-S5 — Crate README + module docs for the discovery layer (scalar half)

**Intent.** Document the scalar half of the discovery layer for an agent landing in
`hdx-core`: extend `crates/core/README.md`'s Mermaid module map + glossary with the new
`layout` / `scalar_reader` / `discovery` modules and the domain terms they introduce,
and tighten module `//!` docs where the MS3 design (the LOW-1 bounded fallback, the
time-extent provenance, the MED-5 hand-off) deserves a single canonical explanation.
Pure docs, no behavior change; placed last so the map reflects the final MS3 shape.
Independently committable (docs-only) and leaves the repo green.

**Changes.**
- `crates/core/README.md` — update the **Mermaid module map** to add `layout`,
  `scalar_reader`, `discovery` and their dependencies on `newtypes`/`field`/`error`;
  add **glossary** rows for: *layout model*, *root rollup*, *basin dir / folder id vs
  in-file `basin_id`*, *time extent* and its **provenance** (`Statistics` vs
  `BoundedColumnScan`), the **LOW-1 bounded fallback** (one-column, metadata/index-tier,
  architecture-§1-compliant — not a chunk decode), and the **MED-5 hand-off**
  (parquet `time` statistics confirmed Rust-side in MS3; mismatch ⇒ regenerate MS2).
  Note the discovery layer is the **scalar half** and that MS4 adds the gridded/geometry
  half.
- `crates/core/src/lib.rs` — ensure the module-map `//!` lists the new modules with a
  one-line purpose each (consistent with the existing MS1 style).
- Light `//!` touch-ups in `layout.rs` / `scalar_reader.rs` / `discovery.rs` only where
  needed to make the README's claims (LOW-1 bound, MED-5 hand-off, gaps-as-facts) point
  at a single canonical doc location. No code/behavior change.

**Test plan.**
- `cargo test --doc` passes (any doc examples compile); existing tests unchanged and
  still pass.
- A reviewer confirms `crates/core/README.md` renders a Mermaid map including the three
  new modules and a glossary covering layout model / root rollup / time-extent
  provenance / LOW-1 bounded fallback / MED-5 hand-off, and that it states the discovery
  layer here is the **scalar half** (MS4 adds the rest).
- **Gate:** `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings`
  pass.

**Acceptance.**
- `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings` all pass.
- `crates/core/README.md` documents the scalar half of the discovery layer: Mermaid
  module map incl. `layout`/`scalar_reader`/`discovery`, and a glossary covering the
  LOW-1 bounded fallback, time-extent provenance, and the MED-5 hand-off.
- No behavior change; docs proportional to complexity (CLAUDE.md docs policy);
  inert/agnostic discipline restated where relevant.
- Spec MUST-checks: none new (docs) — it makes the MS3 discovery foundations
  navigable for MS4/MS5/MS6.
- Patch-bump + stage `Cargo.toml` + conventional commit + `git tag v<version>`.

**Spec refs.** architecture §1 (metadata reads documented), §3.5 (discovery types),
§5 (shared discovery layer), §7 R3 (fallback classification documented); CLAUDE.md
(crate README with Mermaid map + glossary for the complex crate).

**Commit message.** `docs(core): document the scalar discovery layer (modules + glossary)`

---

## Coverage check — every MS3 deliverable & exit criterion is assigned

| MS3 deliverable / exit criterion (milestones.md) | Step |
|---|---|
| `parquet`/`arrow` deps added to `crates/core/Cargo.toml`; **R1-parquet decision recorded** (pure-Rust, no GDAL) | S1 |
| Layout-walk module → typed in-memory layout model (root artifacts + per-basin paths) | S2 |
| Distinguish "absent gridded subtree because no gridded fields" from "missing required file" (L1/L2/L3 foundations) | S2 (records facts) + S4 (gaps-as-facts) |
| **Hidden/dot/OS-cruft ignored** so only HDX-relevant paths discovered (pre-empt MS6 L3 false positive) | S2 |
| Scalar-parquet reader: arrow schema → `Vec<Field>` (scalar quadrants, dtypes via MS1 `parse_dtype`) | S3 |
| Reader: `basin_id` column presence/value | S3 (read) + S4 (paired with folder id) |
| Reader: `time` column descriptor (name, logical type, nullability, sort) | S3 |
| **Per-basin time extent from row-group statistics** (§8) with a **bounded 1-D column-scan fallback** | S3 |
| Fallback **classified under R3** + recorded in reader docs (LOW-1: only `time` column; not a chunk decode; bound stated) | S3 (impl + docs) + S5 (README) |
| **Test: MS2 fixture `time` carries usable Rust-readable statistics AND extent comes from statistics (not fallback)** (MED-5) | S3 |
| MED-5 mismatch ⇒ **regenerate fixture**, never reader workaround — stated in reader docs | S3 (docs) + S5 (README) |
| Typed errors for malformed parquet / missing required columns (named-field `thiserror`) | S1 (`ParquetRead`) + S2 (`LayoutWalk`) + S3 (`MissingScalarColumn`) |
| Tests over `conformance/valid/minimal/`: basins enumerated; scalar schema; `basin_id`; `time` type/sort | S2 (basins) + S3 (schema/`basin_id`/`time`) + S4 (end-to-end) |
| Companion-mask / `{source}_{variable}` scalar fields catalogued as **ordinary** (no special handling) | S3 (no name magic; `_was_filled` in-test scalar column proven ordinary) + S4 (assembled model) |
| Scalar half of the **shared discovery layer** populated from on-disk bytes (one boundary fn) | S4 |
| Spec MUST-checks advanced (discovery foundations; enforced MS6): **L1/L2/L3, I1, T1** | S2 (L1/L2/L3) + S3 (I1, T1) + S4 (assembled) |
| Crate README / module docs (Mermaid map + glossary) for the new layer | S5 |
| `cargo build` + `cargo test` + `cargo clippy --all-targets -- -D warnings` after every step | S1–S5 |
| Bump+tag commit discipline | S1–S5 |
| Inert/agnostic; six-field floor untouched; no derivable/role/transform/semantic/provenance field; no name magic | S1–S5 (scope guard, per step) |
| Metadata + 1-D column reads only — no gridded chunks; geoparquet schema deferred to MS4 | S1–S4 (scope guard); S3 (fallback bound), S2 (geoparquet presence only) |

**Exit-criteria spec MUST-checks (MS3 ADVANCES the discovery foundations; ENFORCED in
MS6):** L1, L2, L3 (layout model + cruft-ignoring walk), I1 (`basin_id` column present),
T1 (`time` column type/sort/non-null discovered). I2/I3 preconditions (folder-vs-in-file
pair + in-file ids) and H1 preconditions (scalar schema) are *surfaced* for MS6. MS3
**enforces no check** and **emits no verdict** — it builds the typed model both verbs
consume. The gridded/geometry half (G1–G3, Geo1, H2, M5, T2 gridded side) is MS4.
