# MS8 — Conformance suite: exhaustive invalids + golden outputs (resolves R2, part 2)

> **Milestone goal (milestones.md MS8).** Complete **R2**: the curated conformance
> suite — the family of invalid fixtures, each pinning **exactly one** violated §14
> check id (the negative matrix MS6 deferred), plus golden outputs — all regenerated
> by the MS2 generator and wired into regression tests. The validator's full
> fail-closed proof across the §14 checklist.
>
> **Scope is fixtures + tests + docs ONLY.** MS8 adds **no** new `check_*` rule, does
> **not** edit `build_report`, does **not** mutate the manifest beyond the M3-rejection
> demo (which is a 7-field JSON string fed to `Manifest::from_json`, not a new field on
> any type), introduces **no** new domain field, and does **no** MS9 (PyO3) work. No
> `regrid`/`clip`/`reduce`. The inert/agnostic discipline holds: no
> transform/role/semantic/provenance anywhere; the manifest stays exactly the six
> fields; `format_version` stays a hard cut.

---

## 0. Grounding — the enforced-vs-skipped reconciliation (read `validate.rs` FIRST)

**This section is load-bearing and grounds the whole milestone.** It is the result of
reading `crates/core/src/validate.rs` (the entry gate `validate` at line 1259, the
`build_report` assembler at 1327, and every `check_*` rule function) **and** the
discovery code that feeds them (`discovery.rs` `discover_basin`, `gridded_discovery.rs`
`discover_basin_gridded` / `assemble_*_catalog`, `geoparquet_reader.rs` `read_outlines`,
`cog_reader.rs`, `zarr_reader.rs`). The exact behavior — not an assumption — determines
which checks can yield `conformant:false` via a one-mutation on-disk fixture.

### 0.1 The three outcome buckets

Every §14 check, when the valid baseline is mutated to violate it, lands in **exactly
one** of three buckets. MS8 builds an on-disk one-violation fixture **only for Bucket
B**.

| Bucket | Meaning for MS8 | The §14 checks in this bucket |
|---|---|---|
| **A — `Err` at entry/discovery** | The mutation makes `validate` return a `ValidateError` **before** (or instead of) producing a report — so there is **no `conformant:false` report**; the negative is asserted as a typed `Err`, not a fail outcome. | **M2** (entry hard cut), **M3**/**M4** (entry-gate boundary parse), **G3** (reader errors `MissingGridGeoref`), **Geo1**-column-missing + **I1**-outlines-missing (reader errors `MissingGeometryColumn`), **T1**-missing-`time`-column (reader errors `MissingScalarColumn`) |
| **B — `ran:fail` (`conformant:false`)** | The mutation makes a `check_*` return `ran:fail`; `validate` produces a report with `conformant:false` and **exactly** that one id failing. **These get the one-violation on-disk fixture.** | **M5**, **L1**, **L2**, **I2**, **I3**, **H1**, **H2**, **T1** (sort/null/dtype/name legs), **G2** |
| **C — R3 skip (still `conformant:true`)** | The check is honestly `skipped` in v0.1 (byte-deep / on-disk-shape-dependent leg). **No enforceable v0.1 negative exists.** A targeted fixture is documented as "reported skipped-with-reason, still conformant", never as a `conformant:false` pin. | **L3** (`check_l3`, validate.rs:850), **M6** rule (b) axis-regularity (`check_m6`, validate.rs:1058), **T2** (`check_t2`, validate.rs:1090) |

> **M1** is the manifest-exists/valid-JSON/version-first entry gate. A truly absent or
> unparseable `manifest.json` is `ValidateError::ManifestUnreadable` / `Manifest` (Bucket
> A); there is no separate on-disk M1 fixture beyond what M2/M3/M4 already exercise. M1 is
> already proven by the MS6 entry-gate tests
> (`entry_gate_reports_unreadable_manifest_for_missing_manifest_json`,
> validate.rs:1526).

### 0.2 Why Bucket A is `Err`, not `ran:fail` — the code facts

- **M2 / M3 / M4** — `validate` (validate.rs:1275) calls `Manifest::from_json(&..)?`
  **before** `discover` (1279). A wrong `format_version`, a 7th field, an empty
  `crs`/`cadence`, or a bad `created_at` returns `ValidateError::Manifest(..)` (the early
  `?`). `build_report` then marks M1–M4 `ran:pass` **only because reaching it means the
  manifest parsed** (validate.rs:1373). So an M3/M4 *negative* is **never** a fail
  outcome through the verb — it is an entry `Err`. (M2 is identical: the MS6 test
  `wrong_format_version_never_reports_conformant_true`, validate.rs:1915, pins this.)
- **G3** — `read_cog_grid` / `read_zarr_grid` **error** `MissingGridGeoref`
  (gridded_discovery.rs:331-332) when a present artifact has no georef; `discover` maps
  that to `ValidateError::Discovery`. `check_g3` (validate.rs:1162) only ever sees a
  `GridInfo` that *already* resolved a CRS, so its `ran:fail` leg (empty-CRS string) is
  not reachable from a real on-disk mutation. Stripping georef ⇒ **Bucket A**.
- **Geo1 / I1-outlines** — `read_outlines` (geoparquet_reader.rs:181) calls
  `require_column` for `basin_id`/`delineation`/`geometry` and **errors**
  `MissingGeometryColumn` (geoparquet_reader.rs:193-195) if any is absent, then
  hard-codes `has_basin_id/has_delineation/has_geometry: true` and
  `partitioned_by_delineation: false` on the `OutlinesInfo` it returns
  (geoparquet_reader.rs:226-230). So `check_geo1`'s `ran:fail` legs (missing column,
  partitioned) and `check_i1`'s outlines leg are **not reachable** through the verb — a
  missing-column outlines is **Bucket A**. (`partitioned_by_delineation` is *always*
  `false`, so the partition-negative is structurally impossible to surface in v0.1.)
- **T1-missing-`time`-column** — `read_scalar_dynamic` errors `MissingScalarColumn`
  (discovery.rs:235); that is **Bucket A**. Only the **sort / nullable / dtype / name**
  legs of `check_t1` (validate.rs:627) are `ran:fail` (Bucket B), and only the *sort*
  leg can be reached by a one-row reorder that the reader still decodes.

### 0.3 The two one-violation-PURITY collisions the prior critique caught (H-1, H-2)

Two Bucket-B fixtures have a non-obvious second-check collision; both are **resolved by
code, not hedged**:

- **L2 must NOT delete a basin's `scalar_dynamic.parquet`.** `discover_basin`
  (discovery.rs:240-253) records an absent `scalar_dynamic` as `BasinScalar { fields:
  Vec::new(), time: None, .. }`. `build_report` feeds `basin.fields()` into
  `fields_by_basin` (validate.rs:1406-1416), and `check_h1` (validate.rs:502) compares
  each basin's scalar schema_key against the first — the **empty** field list of the
  zeroed basin diverges ⇒ **H1 ALSO `ran:fail`**. So deleting `scalar_dynamic` trips
  **L2 AND H1** (T1 is *not* tripped — `check_t1` skips a `None` descriptor,
  validate.rs:631; I1 is *not* tripped — its dynamic leg is gated on `time().is_some()`,
  validate.rs:893). **Resolution (HIGH-1):** the L2 fixture exercises the **gridded
  forward leg** instead — delete one basin's `gridded_static/era5.tif` (the COG),
  keeping `scalar_dynamic.parquet` intact. Verified against the code (see S3 L2 row):
  this trips **only** L2.
- **H2 grid-label rename does NOT trip H1.** `fields_by_basin` (validate.rs:1406) is
  built from `discovery.scalar().per_basin()` → `BasinScalar::fields()`, i.e. the
  **scalar dynamic** fields only. The **gridded** field catalog is *not* an input to
  `check_h1` in v0.1. The grid label is part of `check_h2`'s label set (validate.rs:561)
  but the H1 `schema_key` (validate.rs:519) only sees grid_label for *scalar* fields —
  and scalar fields carry no grid label. So renaming a basin's gridded label changes the
  H2 set but **not** the H1 scalar schema_key ⇒ **H2-only is achievable**.
  **Resolution (HIGH-2):** the concrete H2 mutation is pinned in S3 (relabel one basin's
  grid family) and proven H2-fail-only against `check_h1`/`check_h2`/`check_g2`/`check_g3`
  — not left to ad-hoc "record and adjust".

### 0.4 The companion-mask + `{source}_{variable}` ordinariness (re-asserted, both verbs)

`era5_precipitation` and `era5_precipitation_was_filled` are catalogued as **ordinary**
fields (exactly `{name, quadrant, dtype, units, grid_label}`) with **no** suffix/prefix
special-casing in **both** verbs — already pinned by `describe.rs` (golden snapshot) and
`validate.rs` (`validate_treats_companion_mask_fields_as_ordinary`, validate.rs:2086).
MS8 **re-affirms** this in the consolidated matrix test; it does not add new handling.

---

## Ordering rationale

S1 lands the MS8 **test + generator scaffold** (the Rust regression harness module and
the generator's `MS8Invalid` enum + per-fixture diff expectations) **byte-preservingly**:
the valid baseline and both MS6 goldens are unchanged by construction, so S1 is green with
no new fixtures yet. S2–S4 then add the Bucket-B invalid fixtures **grouped by failure
family**, smallest-blast-radius first, so each step is a small reviewable unit and the
"exactly one id fails" purity is proven incrementally against the code:

- **S2 — manifest/identity family (M5, I2, I3 + the M3/M4 entry-`Err` demos).** The
  manifest-boundary negatives (M3/M4) are pure JSON-string asserts (no on-disk tree);
  M5/I2/I3 are surgical single-value mutations of the scalar/manifest bytes that cannot
  collide with each other.
- **S3 — layout/homogeneity family (L1, L2, H1, H2).** The two collision-prone fixtures
  (L2, H2) live here with their code-verified purity proofs; L1 reuses MS2's
  `missing-root-rollup` (already proven L1-only by MS6).
- **S4 — grids/time/geometry family + Bucket-A demos (G2, T1; G3, Geo1, I1-outlines as
  Err).** G2 (shared-but-misaligned) and T1 (unsorted) are the Bucket-B grids/time
  negatives; G3/Geo1/I1-outlines are the Bucket-A `Err` demos that document why no
  `conformant:false` exists for them.
- **S5 — Bucket-C skip-demo fixtures (L3, M6, T2).** Conformant trees that assert the
  honest-skip outcome; they live under a **separate** `conformance/skip-demo/` directory
  (LOW-2 critique) and a **separate** README sub-table so they are never read as
  negatives.
- **S6 — consolidate: the table-driven matrix test + finalized README.** Re-affirms the
  unchanged goldens, adds the single consolidated matrix regression, and finalizes the
  check-id→fixture documentation.

Dependency flow: S1 (scaffold) → S2/S3/S4/S5 (fixtures, each green on its own) → S6
(consolidation). Every step regenerates deterministically via the MS2 generator
(`conformance/generator/regenerate.sh`, the pinned python3.12 venv) and leaves the repo
green (`cargo build` + `cargo test` + `cargo clippy --all-targets -- -D warnings`).

---

## MS8-S1 — MS8 regression-harness + generator scaffold (byte-preserving)

**id:** MS8-S1

**Intent.** Stand up the MS8 test harness and generator scaffolding **without adding any
fixture yet**, so the milestone's later steps drop fixtures into a ready frame and each
stays a small reviewable unit. Independently committable + green: the valid baseline and
both MS6 goldens are **byte-unchanged** (regenerating yields a bit-for-bit identical tree
per `conformance/README.md` determinism), and the new test module compiles with a
placeholder that asserts the existing two MS2 invalids (already proven by MS6) so the
harness is exercised, not dormant.

**Changes.**
- `conformance/generator/hdx_fixtures/mutate.py` — add an `MS8Invalid` enum (mirroring the
  existing `Invalid` enum, mutate.py:47) carrying, per variant, the folder name, the single
  pinned §14 check id, and the **bucket** (`B` = ran:fail, `A` = entry/discovery `Err`).
  Add the dispatch skeleton `derive_ms8_invalid(baseline_root, repo_root, variant)` that
  copies the baseline and applies the one mutation (no mutations wired yet — empty match
  arms raising `NotImplementedError` are fine for S1; S2–S4 fill them). Keep the existing
  `Invalid` enum and `derive_invalid` untouched.
- `conformance/generator/hdx_fixtures/assertions.py` — add a per-fixture diff-expectation
  helper `assert_ms8_differs_in_exactly_one_way(baseline_root, invalid_root, variant)` that
  generalizes the existing `assert_differs_in_exactly_one_way` (assertions.py:754) to a
  table of `(added, removed, changed)` expectations keyed by `MS8Invalid` — so a
  many-file Zarr-directory rename (H2) and a single-value byte change (M5/I2/I3) are both
  expressible. No expectations populated yet.
- `conformance/generator/hdx_fixtures/build.py` — add a `derive_ms8_invalids(dataset_root)`
  hook (no-op when the `MS8Invalid` table is empty) and call it after `derive_invalids`
  (build.py:109), behind the same abort-on-self-assertion-failure contract.
- `crates/core/src/validate.rs` (test module only) — add an MS8 regression sub-module
  `mod ms8_conformance_matrix` with: a `fn ms8_fixture(name: &str) -> PathBuf` resolver
  (under a new `conformance/invalid-ms8/` root), and a placeholder table-driven test that,
  for now, asserts the two **existing** MS2 invalids
  (`invalid/missing-root-rollup` → L1-only fail; `invalid/wrong-format-version` → entry
  `Err`) via the same assertion shape S6 will use for the full matrix. No production code
  changes.
- `conformance/README.md` — add an **MS8 section stub** stating the three-bucket model
  (§0.1 here), the `invalid-ms8/` (Bucket B) and `skip-demo/` (Bucket C) directory split,
  and that the table is filled by S2–S6.

**Test plan.**
- `cargo test -p hdx-core ms8_conformance_matrix` — the placeholder table-driven test runs
  against the two existing MS2 invalids and passes (proves the harness shape works).
- `conformance/generator/regenerate.sh` — runs end-to-end, byte-identical output (the
  determinism contract); the empty `MS8Invalid` table is a no-op; all existing
  self-assertions still pass.
- `git status` after regenerate shows **no diff** to `valid/minimal/**`,
  `valid/minimal/describe.golden.json`, or `valid/minimal/validate.golden.json`.

**Acceptance.**
- `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings` all pass.
- The two MS6 goldens and the valid baseline are byte-unchanged (S1 adds no fixture).
- The placeholder matrix test `cargo test -p hdx-core ms8_conformance_matrix` passes.
- `MS8Invalid` enum + per-fixture diff-expectation helper + build hook exist and are
  exercised (no dead code; clippy clean).
- README MS8 stub records the three-bucket split (Bucket A `Err`, Bucket B `ran:fail`,
  Bucket C R3-skip) and the `invalid-ms8/` vs `skip-demo/` directory plan.
- Advances spec-check coverage: re-confirms **L1** (MS2 missing-rollup, ran:fail-only)
  and **M2** (entry `Err`) through the new harness shape; no new check enforced.

**Spec refs.** §14 (the checklist + the enforcement-depth note), §10/R2 (dev-only
generator), architecture §6 (conformance suite), §7 R2/R3.

**commit_message:** `test(ms8): scaffold conformance matrix harness + generator MS8Invalid frame`

---

## MS8-S2 — Manifest + identity Bucket-B invalids (M5, I2, I3) + M3/M4 entry-Err demos

**id:** MS8-S2

**Intent.** Add the first family of one-violation invalids — the manifest-CRS and
identity negatives — each a surgical single-value mutation that the code proves cannot
collide with a second check; plus the M3/M4 boundary-`Err` demos as pure JSON-string
asserts (Bucket A, no on-disk tree). Independently committable + green.

**Changes.**
- `conformance/generator/hdx_fixtures/mutate.py` — implement three `MS8Invalid` arms,
  each one surgical mutation off the baseline:
  - `M5_CRS_MISMATCH` (pins **M5**, Bucket B): rewrite the **manifest** `crs` to
    `"EPSG:3857"` while every file's recorded CRS stays `EPSG:4326`. (Mutating the
    manifest value — not a file — is the cleanest single change; `check_m5`,
    validate.rs:970, compares each `GridInfo.crs()` and the outlines CRS to
    `manifest.crs()`.) One file changes: `manifest.json` (the `crs` value only).
  - `I2_FOLDER_MISMATCH` (pins **I2**, Bucket B): in **one** basin's
    `scalar_dynamic.parquet`, rewrite the in-file `basin_id` column to a value
    (`"9999"`) that disagrees with its `basin=<id>` folder. `check_i2` (validate.rs:934)
    compares folder vs in-file; `check_i3` still sees distinct values so it does not
    also fail; H1 (scalar schema by name/quadrant/dtype, not by value) is unaffected.
  - `I3_DUPLICATE_BASIN_ID` (pins **I3**, Bucket B): rewrite the **root**
    `scalar_static.parquet` so two rows carry the same `basin_id` (and one basin's
    `scalar_dynamic` in-file id matched accordingly so the in-file id stream
    `in_file_basin_ids`, validate.rs:1439, has a duplicate). `check_i3`
    (validate.rs:600) fails on the first repeat; I2 stays pass (folder still agrees for
    the value that is duplicated); L1/L2 untouched.
- `conformance/generator/hdx_fixtures/assertions.py` — populate the diff-expectation rows
  for the three variants (M5/I2/I3 each: `added={}`, `removed={}`, `changed={one file}`).
- `crates/core/src/validate.rs` (test module) — add three named regression tests:
  - `m5_crs_mismatch_pins_exactly_m5` — `validate(invalid-ms8/m5-crs-mismatch)` →
    `conformant:false`, M5 `ran:fail`, every other check pass-or-skip.
  - `i2_folder_mismatch_pins_exactly_i2` — likewise for I2.
  - `i3_duplicate_basin_id_pins_exactly_i3` — likewise for I3.
  - `m3_seven_field_manifest_is_entry_err` / `m4_empty_crs_is_entry_err` — assert the
    **Bucket A** form: feed the on-disk mutated `manifest.json` (or a hand-built JSON
    string) through `validate`/`Manifest::from_json` and assert
    `Err(ValidateError::Manifest(CoreError::ExtraManifestField{..}))` resp.
    `Err(..EmptyCrs)`. **Citation (MEDIUM-fix):** state in the test/README that the
    empty-crs / empty-cadence / bad-created-at M4 legs are *already* pinned by the
    retained `m4_in_memory_negative_empty_crs_and_bad_created_at_rejected`
    (validate.rs:1561) and M3 by `m3_in_memory_negative_seven_field_manifest_rejected`
    (validate.rs:1542) — so "M3/M4 covered" is verifiable, not a blanket claim.

**Test plan.**
- `cargo test -p hdx-core m5_crs_mismatch_pins_exactly_m5`,
  `cargo test -p hdx-core i2_folder_mismatch_pins_exactly_i2`,
  `cargo test -p hdx-core i3_duplicate_basin_id_pins_exactly_i3` — each passes.
- `cargo test -p hdx-core m3_seven_field_manifest_is_entry_err`,
  `cargo test -p hdx-core m4_empty_crs_is_entry_err` — each passes (Bucket A `Err`).
- `regenerate.sh` emits the three new Bucket-B trees under `conformance/invalid-ms8/`
  deterministically; each generator self-assertion (differs-in-exactly-one-way) passes.

**Acceptance.**
- `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings` all pass.
- Each Bucket-B fixture yields `conformant:false` with **exactly** its one id `ran:fail`
  and all others pass-or-skip (the named test asserts the full report, not just the one
  check).
- M3/M4 negatives are asserted as **entry `Err`** (Bucket A) with the documented citation
  to the retained MS6 boundary tests; no claim of a `conformant:false` for M3/M4.
- Fixtures generated programmatically (one mutation each); no hand-edit; generator
  self-assertions pass; deterministic regenerate.
- Advances **M5, I2, I3** (Bucket-B negatives) and documents **M3, M4** (Bucket-A `Err`).

**Spec refs.** §3 (identity I1–I3), §7/§11 (M5 crs cross-check), §11/§14 M3–M4 (boundary
parse), §14 (one-violation discipline), R2.

**commit_message:** `test(ms8): add M5/I2/I3 one-violation invalids + M3/M4 entry-Err demos`

---

## MS8-S3 — Layout + homogeneity Bucket-B invalids (L1, L2, H1, H2) — the collision-proof step

**id:** MS8-S3

**Intent.** Add the layout/homogeneity one-violation family, including the **two
collision-prone fixtures (L2, H2)** with their code-verified purity proofs (the HIGH-1
and HIGH-2 resolutions). Each fixture is proven `ran:fail`-on-exactly-one against the
actual `check_*` + discovery code. Independently committable + green.

**Changes.**
- `conformance/generator/hdx_fixtures/mutate.py` — implement four `MS8Invalid` arms:
  - `L1_MISSING_ROOT_ROLLUP` (pins **L1**, Bucket B): **reuse** MS2's mutation — delete
    the root `outlines.geoparquet`. Already proven L1-only by MS6
    (`missing_root_rollup_pins_exactly_l1_and_is_non_conformant`, validate.rs:1950); the
    MS8 matrix references it (Geo1 honestly skips when outlines is absent — that is L1's
    job, not a second fail). Either reuse the existing `invalid/missing-root-rollup/`
    tree directly in the matrix or re-derive it under `invalid-ms8/` — pick one and state
    it (recommend: reference the existing tree to avoid a duplicate).
  - `L2_MISSING_GRIDDED_STATIC` (pins **L2**, Bucket B; **HIGH-1 resolution**): delete
    **one** basin's `gridded_static/era5.tif` (the COG), keeping its
    `scalar_dynamic.parquet`, its `gridded_dynamic/era5.zarr`, and the homogeneous label
    set intact. **Code-verified purity:** `check_l2` gridded-static forward leg
    (validate.rs:811) fails because the schema declares `GriddedStatic` `elevation` but
    that basin's `static_artifacts()` is empty. It does **not** trip:
    - **H1** — `fields_by_basin` is **scalar-only** (validate.rs:1406, `BasinScalar::fields()`);
      deleting a COG leaves all basins' scalar schema `[streamflow]` identical → H1 pass.
    - **H2** — the mutated basin's label set becomes `{}` (static) ⊕ `{era5}` (dynamic) =
      `{era5}`, identical to the reference basin's `{era5}` (a *set*, validate.rs:562) →
      H2 pass.
    - **G2** — `check_g2` (validate.rs:1113) iterates `static_artifacts()`; the mutated
      basin has none, so its loop body is skipped; other basins' shared `era5` still
      coincides → G2 pass.
    - **G1/G3/M5** — the gridded catalog/grids are built from the *first* basin exposing
      an artifact (assemble_gridded_field_catalog, gridded_discovery.rs:394) and the
      union of observed `GridInfo`s; one fewer COG leaves self-naming, georef, and crs
      intact → pass.
    The **L2↔H1 collision the prior critique flagged is named explicitly** and avoided by
    not zeroing a basin's scalar schema.
  - `H1_DIVERGENT_SCALAR_SCHEMA` (pins **H1**, Bucket B): in **one** basin's
    `scalar_dynamic.parquet`, change the `streamflow` column **dtype** (e.g. float64 →
    float32) so that basin's scalar `schema_key` (validate.rs:519, the
    `(name,quadrant,dtype,grid_label)` tuple) diverges. `check_h1` fails; **purity:** the
    column name/quadrant are unchanged so I1 (basin_id presence), T1 (the `time` column is
    untouched), L2 (scalar_dynamic still present) all pass; the value-count/sort is
    unchanged. (If `parse_dtype` rejects the chosen physical type, pick a mapped one — the
    test asserts discovery *succeeds* first, then H1 fails.)
  - `H2_DIVERGENT_GRID_LABEL` (pins **H2**, Bucket B; **HIGH-2 resolution**): relabel
    **one** basin's grid family from `era5` to `chirps` by renaming **both** that basin's
    `gridded_static/era5.tif` → `chirps.tif` **and** `gridded_dynamic/era5.zarr` →
    `chirps.zarr` (keeping the two mutually cell-for-cell aligned, so the shared `chirps`
    label still coincides). **Code-verified purity:**
    - **H2** — that basin's label set becomes `{chirps}` vs the reference `{era5}`
      (validate.rs:562) → H2 `ran:fail`. ✓
    - **H1** — `check_h1` is **scalar-only** (validate.rs:1406); a *gridded* relabel does
      not enter the scalar `schema_key`, and scalar fields carry no grid label → H1 pass.
      **This is the concrete code fact that makes an H2-only mutation possible** (the
      prior critique's "the grid label feeds both rules" concern does not hold for v0.1's
      scalar-only H1 input). ✓
    - **G2** — the relabeled basin's COG+Zarr **both** carry `chirps` and remain aligned,
      so `check_g2` (validate.rs:1113) finds the shared `chirps` label coincides → G2
      pass. ✓ (If only one of the pair were renamed, the basin would have no shared label
      and G2 would still pass via `continue` — but renaming both keeps the relabel a clean
      single grid-family relabel.)
    - **G3** — both renamed artifacts keep their georef → `check_g3` (validate.rs:1162)
      pass. ✓
    - **M5** — the renamed artifacts keep `EPSG:4326` → `check_m5` pass. ✓
    Because the Zarr is a directory of many files, the rename touches many paths; the S1
    per-fixture diff-expectation helper expresses this as `removed={era5.zarr/** ,
    era5.tif}`, `added={chirps.zarr/**, chirps.tif}`, `changed={}` — a single
    *conceptual* relabel mutation (LOW-2 spirit: one targeted change, derived not
    hand-edited). State the H2↔H1 interaction explicitly in the fixture's docstring with
    the code citation.
- `conformance/generator/hdx_fixtures/assertions.py` — populate the four diff-expectation
  rows.
- `crates/core/src/validate.rs` (test module) — add named regression tests:
  - `l2_missing_gridded_static_pins_exactly_l2` — full-report assertion: L2 `ran:fail`,
    H1/H2/G2/G1/G3/M5 and all others pass-or-skip. **The test body comments the L2↔H1
    non-collision** (scalar-only H1).
  - `h1_divergent_scalar_dtype_pins_exactly_h1` — H1 `ran:fail`, others pass-or-skip.
  - `h2_divergent_grid_label_pins_exactly_h2` — H2 `ran:fail`, **H1 pass** (the scalar-only
    fact), G2 pass (shared `chirps` aligned), G3/M5 pass, all others pass-or-skip.
  - L1 is asserted via the existing MS6 test referenced in the matrix (no new tree).

**Test plan.**
- `cargo test -p hdx-core l2_missing_gridded_static_pins_exactly_l2` — passes; the
  full-report loop confirms **only** L2 is `ran:fail`.
- `cargo test -p hdx-core h1_divergent_scalar_dtype_pins_exactly_h1` — passes.
- `cargo test -p hdx-core h2_divergent_grid_label_pins_exactly_h2` — passes; explicitly
  asserts `report.find(H1).result() == Some(Pass)` and `report.find(G2).result() ==
  Some(Pass)` to prove the non-collisions.
- `regenerate.sh` emits the L2/H1/H2 trees deterministically; each generator
  self-assertion (per-fixture diff expectation) passes; the L2 fixture's
  `scalar_dynamic.parquet` is byte-identical to baseline (proving H1 cannot be tripped).

**Acceptance.**
- `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings` all pass.
- **L2 fixture proven L2-`ran:fail`-only** (HIGH-1): the gridded-static delete leaves the
  scalar schema and label set homogeneous; the named test's full-report loop is green.
- **H2 fixture proven H2-`ran:fail`-only** (HIGH-2): the concrete grid-family relabel is
  code-verified H2-only (H1 scalar-only; G2 shared-aligned; G3/M5 intact); no "record and
  adjust" hedge remains — the mutation is pinned and the test asserts H1/G2 pass.
- H1 fixture proven H1-`ran:fail`-only; L1 referenced via the existing MS6 test.
- Fixtures generated programmatically (one targeted mutation each); deterministic
  regenerate; no hand-edit.
- Advances **L1, L2, H1, H2** (Bucket-B negatives).

**Spec refs.** §4 (layout L1–L2), §5 (homogeneity H1), §8 (grid-label set H2), §14
(one-violation discipline), R2.

**commit_message:** `test(ms8): add L2/H1/H2 one-violation invalids (collision-proof) + L1 ref`

---

## MS8-S4 — Grids/time Bucket-B (G2, T1) + the Bucket-A entry-Err demos (G3, Geo1, I1-outlines)

**id:** MS8-S4

**Intent.** Add the grids/time one-violation negatives that *are* `ran:fail` (G2
shared-but-misaligned, T1 unsorted), and the **Bucket-A** demos (G3, Geo1, I1-outlines)
that document — with a code-verified error variant — why no `conformant:false` exists for
them in v0.1. Independently committable + green. **No `check_*` / `build_report` edit:**
if a Bucket-A probe surfaces a genuine validator behavior gap, the step **halts as a
finding** (out of MS8 scope), it does not patch rule logic.

**Changes.**
- `conformance/generator/hdx_fixtures/mutate.py` — implement:
  - `G2_SHARED_BUT_MISALIGNED` (pins **G2**, Bucket B): in **one** basin, keep the shared
    `era5` label on **both** COG and Zarr but write the Zarr `lat`/`lon` (or the COG
    tiepoint) so the two **do not** coincide (shift the extent by one cell, or change the
    pixel count). `check_g2` (validate.rs:1127) compares `extent/resolution/width/height`
    for the shared label and fails when they differ. **Purity:** the label is still
    `{era5}` on both subtrees (H2 set unchanged → H2 pass); the field catalog still
    self-names (G1 pass); both artifacts keep georef + crs (G3, M5 pass); H1 scalar
    unaffected. The misalignment is the *only* divergence.
  - `T1_UNSORTED_TIME` (pins **T1**, Bucket B): in **one** basin's
    `scalar_dynamic.parquet`, reorder the rows so the `time` column is **not** sorted
    ascending (still named `time`, still `Timestamp`, still non-nullable — so the reader
    decodes it). `check_t1` (validate.rs:667) fails the sort leg. **Pin the sort leg
    exactly (MEDIUM-fix):** the test asserts the fixture **discovers successfully first**
    (a missing column → `MissingScalarColumn` Bucket A; a mistyped dtype the reader
    rejects → `UnknownDtype` Bucket A — neither is what we want), then `check_t1`
    `ran:fail`. **Generator self-assertion exclusion (MEDIUM-fix):** the baseline-side
    `assert_time_column_and_statistics` (assertions.py:79) asserts sortedness and would
    abort if applied to the T1 fixture — so the scalar time/sort self-assertions run
    **only on the valid baseline**, never on the derived T1 invalid; state this in the
    T1 fixture docstring and the assertions module.
- `conformance/generator/hdx_fixtures/mutate.py` (Bucket-A demo mutations) — implement,
  each tagged bucket `A`:
  - `G3_MISSING_GEOREF` (documents **G3** Bucket A): pick **one** concrete mutation —
    **strip the Zarr `grid_mapping`/`crs` member** from one basin's `era5.zarr`. **Assert
    the exact variant (MEDIUM-fix):** `read_zarr_grid` errors `MissingGridGeoref`
    (gridded_discovery.rs:332), so `validate` returns
    `Err(ValidateError::Discovery(CoreError::MissingGridGeoref{..}))`. **Drop the
    COG-disjunction** — the COG path uses distinct `CogRead`/MED-4 semantics and is *not*
    the same error; the README documents the Zarr strip as the single asserted G3 Bucket-A
    fixture. (If a separate COG-georef fixture is later wanted, it is its own fixture with
    its own asserted variant — out of MS8-S4 scope.)
  - `GEO1_MISSING_COLUMN` (documents **Geo1** + **I1-outlines** Bucket A): drop the
    `delineation` column from `outlines.geoparquet`. `read_outlines` errors
    `MissingGeometryColumn` (geoparquet_reader.rs:194) → `validate` returns
    `Err(ValidateError::Discovery(CoreError::MissingGeometryColumn{..}))`. Document that
    `partitioned_by_delineation` is *always* `false` (geoparquet_reader.rs:230), so the
    partition-negative is structurally unrepresentable in v0.1 (an honest no-negative
    finding), and that a missing `basin_id` column in outlines is the **same** Bucket-A
    `Err` (I1-outlines leg), not a `check_i1` fail.
- `conformance/generator/hdx_fixtures/assertions.py` — populate diff-expectation rows for
  G2/T1/G3/Geo1; ensure the scalar time/sort self-assertions are gated to the baseline only.
- `crates/core/src/validate.rs` (test module) — add named tests:
  - `g2_shared_misaligned_pins_exactly_g2` — G2 `ran:fail`, H2/G1/G3/M5/H1 pass-or-skip.
  - `t1_unsorted_time_pins_exactly_t1` — fixture **discovers OK**, then T1 `ran:fail`,
    others pass-or-skip.
  - `g3_missing_zarr_georef_is_entry_err` — `Err(Discovery(MissingGridGeoref{..}))`.
  - `geo1_missing_delineation_column_is_entry_err` — `Err(Discovery(MissingGeometryColumn{..}))`.
- `architecture.md` — append **one** new row to the §8 Amendments table (newest-first, with
  the date, per the table's own convention at lines 306-310) recording the MS8 Bucket-A
  finding: G3/Geo1/I1-outlines have **no v0.1 `conformant:false`** because the readers fail
  closed at discovery (typed `Err`), and `partitioned_by_delineation` is always `false`.
  **Scope guard:** the amendment is appended to the table (not inlined into the body); any
  Bucket-A probe that surfaces a real `check_g1`/`check_geo1`/`build_report` behavior gap is
  recorded as a **finding and halts the step** — MS8 does not patch rule logic.

**Test plan.**
- `cargo test -p hdx-core g2_shared_misaligned_pins_exactly_g2` — passes.
- `cargo test -p hdx-core t1_unsorted_time_pins_exactly_t1` — passes (discovers, then T1
  fails).
- `cargo test -p hdx-core g3_missing_zarr_georef_is_entry_err`,
  `cargo test -p hdx-core geo1_missing_delineation_column_is_entry_err` — each asserts the
  exact `ValidateError::Discovery(..)` variant.
- `regenerate.sh` emits G2/T1/G3/Geo1 trees deterministically; the T1 fixture is **not**
  subjected to the baseline sort self-assertion; all gating self-assertions pass.

**Acceptance.**
- `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings` all pass.
- **G2** (shared-but-misaligned) and **T1** (unsorted, discovers-first) are proven
  one-id-`ran:fail`-only.
- **G3** Bucket-A asserts **one** concrete variant (`MissingGridGeoref` from the Zarr
  grid_mapping strip); the COG disjunction is dropped.
- **Geo1 / I1-outlines** Bucket-A asserts `MissingGeometryColumn`; the always-`false`
  partition fact is documented as a no-negative finding.
- The architecture.md amendment is **appended to the §8 table** (newest-first, dated); no
  `check_*`/`build_report` change; any rule-gap is a halting finding, not a patch.
- Advances **G2, T1** (Bucket-B negatives); documents **G3, Geo1, I1-outlines** (Bucket-A
  `Err`).

**Spec refs.** §6 (T1 sort), §7/§8 (G2 alignment, G3 georef), §9 (Geo1 outlines), §14
note (report which ran), R2/R3.

**commit_message:** `test(ms8): add G2/T1 one-violation invalids + G3/Geo1/I1 entry-Err demos`

---

## MS8-S5 — Bucket-C skip-demo fixtures (L3, M6, T2) under a separate directory

**id:** MS8-S5

**Intent.** Add the three Bucket-C **skip-demonstration** fixtures (L3, M6 axis-regularity,
T2) as **conformant** trees that assert the honest-skip outcome — never a
`conformant:false` pin. Per the LOW-2 critique they live under a **separate**
`conformance/skip-demo/` directory and a **separate** README sub-table so a future agent
never reads them as negatives. Independently committable + green.

**Changes.**
- `conformance/generator/hdx_fixtures/mutate.py` — add `MS8SkipDemo` variants (bucket `C`),
  derived into `conformance/skip-demo/<name>/`:
  - `M6_IRREGULAR_AXIS` (demonstrates **M6**, MED-1 resolution): a basin whose realized
    `time` axis is **internally irregular** (a non-constant interior step). **Expected
    outcome: `conformant:true`** — `check_m6` (validate.rs:1058) rule (a) cadence-non-empty
    passes and rule (b) axis-regularity is **R3 `Skipped`-with-reason** (the two-point
    `[start,end]` extent cannot prove a constant interior step; validate.rs:1070). There is
    **no enforceable M6 negative in v0.1**; the cross-basin "same step" rule is **NOT**
    resurrected (it was dropped per MED-1). The fixture proves the skip is reported, not a
    fail. (This fixture may be byte-equal to the baseline if the baseline's axis already
    can't prove regularity — in that case state that M6 needs no distinct tree and the
    baseline golden already demonstrates the skip; the M6 skip is already pinned by
    `m6_on_valid_fixture_is_not_a_fail_and_names_the_regularity_leg`, validate.rs:1988.)
  - `L3_*` and `T2_*` skip demos: similarly, document that `check_l3` (validate.rs:850) and
    `check_t2` (validate.rs:1090) are **unconditional R3 skips** in v0.1 (their byte-deep
    legs are never run), so **any** conformant tree already demonstrates the skip — there
    is no fixture that flips them to `ran:fail`. The S5 fixtures (or a documented reuse of
    the baseline) assert: `report.find(L3/T2/M6).status() == Skipped`, `result() == None`,
    a non-empty `detail`, and `report.conformant() == true`.
- `conformance/generator/hdx_fixtures/assertions.py` — diff-expectation rows for any
  distinct skip-demo trees; for reused-baseline demos, no new tree (state it).
- `crates/core/src/validate.rs` (test module) — add named tests asserting the **skip**
  outcome (NOT a negative):
  - `m6_irregular_axis_is_skipped_not_failed_and_still_conformant`
  - `l3_is_unconditional_r3_skip_on_conformant_tree`
  - `t2_is_unconditional_r3_skip_on_conformant_tree`
  Each asserts `status==Skipped`, `result()==None`, non-empty `detail`, `conformant()==true`.
- `conformance/README.md` — add a **separate "Bucket-C skip demonstrations" sub-table**
  (clearly distinct from the negative check-id table) listing L3/M6/T2, each with: the
  `skip-demo/` location, the expected `skipped`+`conformant:true` outcome, and the
  statement "no enforceable v0.1 negative exists for this check".

**Test plan.**
- `cargo test -p hdx-core m6_irregular_axis_is_skipped_not_failed_and_still_conformant` —
  passes (skip, not fail; conformant true).
- `cargo test -p hdx-core l3_is_unconditional_r3_skip_on_conformant_tree`,
  `cargo test -p hdx-core t2_is_unconditional_r3_skip_on_conformant_tree` — each passes.
- `regenerate.sh` emits any distinct `skip-demo/` trees deterministically; the README
  sub-table is the only place these are listed.

**Acceptance.**
- `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings` all pass.
- L3/M6/T2 are documented + tested as **R3 skip-with-reason, dataset still conformant** —
  **never** a `conformant:false` claim; the dropped cross-basin same-step M6 rule is not
  resurrected.
- The skip-demo fixtures live under `conformance/skip-demo/` with a **separate** README
  sub-table; they are never listed in the negative check-id→fixture table.
- Advances honest-skip coverage for **L3, M6, T2**; no new enforced check.

**Spec refs.** §1/§6.4 (no cadence semantics — M6 MED-1), §6.2 (T2 axis identity), §5/§14
L3 (absence-is-NaN), §14 note (report skips honestly), R3.

**commit_message:** `test(ms8): add L3/M6/T2 skip-demo fixtures (separate dir, conformant)`

---

## MS8-S6 — Consolidate: table-driven matrix test + finalized README

**id:** MS8-S6

**Intent.** Consolidate the milestone into the single deliverable the reviewer runs: a
table-driven matrix regression over **all** MS8 fixtures (Bucket B `ran:fail`-once, Bucket
A `Err`, Bucket C skip), and a finalized `conformance/README.md` check-id→fixture table.
**Re-affirms** (does not re-create) the two MS6 goldens and the dual-verb ordinariness —
those already exist from MS5/MS6 and the valid baseline is byte-unchanged by design (LOW
critique). Independently committable + green.

**Changes.**
- `crates/core/src/validate.rs` (test module) — finalize the `ms8_conformance_matrix`
  table-driven test introduced in S1: a single static table of
  `(fixture_name, pinned_check_id, bucket)` rows covering every MS8 fixture, with a loop
  that, per row, asserts:
  - **Bucket B** → `validate(fixture)` is `Ok`, `conformant()==false`, the pinned id is
    `ran:fail`, **every other id** is pass-or-skip (the one-violation purity, exhaustively).
  - **Bucket A** → `validate(fixture)` is the expected `Err(ValidateError::..(..))` variant.
  - **Bucket C** → `validate(fixture)` is `Ok`, `conformant()==true`, the pinned id is
    `Skipped` with a non-empty `detail`.
  Plus a **positive row**: the valid baseline → `conformant:true`, no `ran:fail`.
- `crates/core/src/validate.rs` + `crates/core/src/describe.rs` (test module) — **re-affirm**
  (do not duplicate) the dual-verb companion-mask/`{source}_{variable}` ordinariness: a
  single consolidated assertion (or a comment cross-referencing
  `validate_treats_companion_mask_fields_as_ordinary`, validate.rs:2086, and the describe
  golden) that both verbs catalog the two fields as ordinary `{name,quadrant,dtype,units,
  grid_label}` with no special key.
- `conformance/README.md` — finalize the **negative check-id → invalid-fixture table**
  (Bucket B + Bucket A rows, each: check id, fixture, the one mutation, the bucket), keep
  the **separate Bucket-C skip sub-table** from S5, document the **golden-update workflow**
  (re-stating the MS5/MS6 "regenerate from the Rust verb only on a `format_version` bump"
  rule), and the **regenerate workflow** + dev-only-generator rule. State the
  **enforced-vs-skipped split** explicitly: Bucket B = enforced `conformant:false`; Bucket
  C = R3-skipped (no v0.1 negative); Bucket A = fail-closed entry/discovery `Err`.
- (No golden file is rewritten — the valid baseline is byte-unchanged, so
  `describe.golden.json` / `validate.golden.json` and their existing snapshot + schema
  tests are **re-affirmed**, not extended; the matrix test is the genuinely new content.)

**Test plan.**
- `cargo test -p hdx-core ms8_conformance_matrix` — the finalized table-driven test passes
  over every MS8 fixture (Bucket B/A/C) + the positive baseline row.
- `cargo test -p hdx-core` (whole crate) — all existing MS5/MS6 golden snapshot + schema
  tests still pass unchanged (byte-unchanged goldens).
- `regenerate.sh` — full deterministic regenerate of the whole fixture family (valid +
  all `invalid-ms8/` + `skip-demo/`) with every self-assertion passing; `git status` shows
  no diff to the two committed goldens or the valid baseline.

**Acceptance.**
- `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings` all pass.
- The single `ms8_conformance_matrix` test exhaustively asserts one-violation purity for
  every Bucket-B fixture, the exact `Err` for every Bucket-A demo, and the skip-with-reason
  for every Bucket-C demo, plus `conformant:true` on the baseline.
- README finalizes the negative table + the separate skip sub-table + the documented
  golden-update workflow, and states the enforced-vs-skipped split explicitly.
- The two MS6 goldens and the valid baseline are **re-affirmed byte-unchanged** (S6 does
  not rewrite them); the matrix test is the new deliverable (no overclaim of "extending"
  the goldens).
- R2 fully resolved: every applicable §14 check has a positive (valid fixture) + a
  bucket-appropriate negative/skip regression, all regenerated deterministically by the
  dev-only generator.

**Spec refs.** §14 (every check id, positive + negative/skip), §2 (companion-mask ordinary
in both verbs), §10/R2 (dev-only generator), architecture §6, R2/R3.

**commit_message:** `test(ms8): consolidate conformance matrix + finalize README + re-affirm goldens`

---

## Coverage check — every §14 check has its MS8 outcome

| §14 check | Bucket | MS8 fixture / demo | Step |
|---|---|---|---|
| M1 | A | covered by MS6 entry-gate tests (unreadable/absent manifest) | — (ref) |
| M2 | A | `wrong-format-version` (MS2) → entry `Err` | S1 (ref) |
| M3 | A | 7-field JSON string → `ExtraManifestField` `Err` | S2 |
| M4 | A | empty crs / bad created_at → `EmptyCrs` / `InvalidTimestamp` `Err` (ref MS6 in-memory) | S2 |
| M5 | B | `m5-crs-mismatch` (manifest crs ≠ files) → M5 ran:fail | S2 |
| L1 | B | `missing-root-rollup` (MS2) → L1 ran:fail | S3 (ref) |
| L2 | B | `l2-missing-gridded-static` (delete one COG) → L2 ran:fail only | S3 |
| H1 | B | `h1-divergent-scalar-dtype` → H1 ran:fail only | S3 |
| H2 | B | `h2-divergent-grid-label` (relabel grid family) → H2 ran:fail only | S3 |
| I1 | A | missing basin_id in outlines → `MissingGeometryColumn` `Err` | S4 |
| I2 | B | `i2-folder-mismatch` → I2 ran:fail | S2 |
| I3 | B | `i3-duplicate-basin-id` → I3 ran:fail | S2 |
| T1 | B | `t1-unsorted-time` (discovers, then sort leg) → T1 ran:fail | S4 |
| G1 | — | self-naming holds by construction; missing-band-name is COG `R3Skip`, not a fail — documented, no negative | S4 (doc) |
| G2 | B | `g2-shared-but-misaligned` → G2 ran:fail only | S4 |
| G3 | A | strip Zarr grid_mapping → `MissingGridGeoref` `Err` | S4 |
| Geo1 | A | drop `delineation` column → `MissingGeometryColumn` `Err`; partition always-false (no negative) | S4 |
| L3 | C | unconditional R3 skip, conformant:true | S5 |
| M6 | C | irregular axis → rule(b) R3 skip, conformant:true (no negative; cross-basin rule NOT resurrected) | S5 |
| T2 | C | unconditional R3 skip, conformant:true | S5 |

> **G1** has no clean one-violation negative in v0.1: `Field::new` makes a label-less
> gridded field unrepresentable and the COG reader records a missing band description as
> `CogBandSource::R3Skip` (cog_reader.rs) rather than a fail. S4 documents this as a
> no-negative finding (the positive G1 path is proven on the valid fixture by MS6).

---

## Scope guard

No step exceeds MS8 or performs a later milestone's work. MS8 adds **fixtures + tests +
docs only**: it introduces **no** new `check_*` rule, never edits `build_report` or any
`check_*` logic (a Bucket-A probe that reveals a rule gap halts as a finding — it is not
patched), never adds `regrid`/`clip`/`reduce`, performs no MS9 (PyO3) work, and rewrites
neither MS6 golden (the valid baseline is byte-unchanged by the determinism contract). The
inert/agnostic discipline holds throughout: no type or field anywhere carries
transform/role/semantic/provenance; the manifest stays **exactly** the six floor fields
(the M3 negative is a 7-field JSON *string* fed to `Manifest::from_json`, not a new field
on any type); `format_version` stays a hard cut. Bucket-C checks (L3, M6 axis-regularity,
T2) are documented as R3 skip-with-reason with **no** `conformant:false` claim and the
dropped M6 cross-basin same-step rule is **not** resurrected; Bucket-A checks (G3, Geo1,
I1-outlines) are documented as fail-closed entry/discovery `Err` with the exact code-verified
variant, with **no** `conformant:false` claim.
