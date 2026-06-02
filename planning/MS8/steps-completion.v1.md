# MS8 completion plan — remaining steps under the gitignored-fixtures + golden-relocation policy

> **Scope.** This plan covers ONLY the four remaining MS8 work items, ordered and
> dependency-sequential. Everything in "Already done + committed" below is NOT
> re-planned. `hdx-core` is FROZEN for MS8 (no reader / rule / domain-type /
> manifest-floor change; no regrid/clip/reduce). Fixtures + tests + goldens + docs
> only.

## 0. Ground truth read before planning

Read and grounded against: `spec/HDX_SPEC.md` §14 (the 20-check checklist),
`architecture.md` (§5 mapping + Amendments log), `conformance/README.md`
(tracking policy + check-id tables + three rules), `crates/core/src/validate.rs`
(the actual ran-vs-skipped behaviour of every check), the generator
(`build.py`, `mutate.py`, `grids.py`, `assertions.py`, `manifest.py`,
`scalar.py`), and the golden tests in `describe.rs` / `validate.rs`.

### The exact problem the policy lesson names (verified on disk)

Root `.gitignore` ignores `conformance/valid/**` and `conformance/invalid/**`,
then re-includes goldens with `!conformance/**/*.golden.json`. So the 7 tracked
goldens physically live **inside the gitignored fixture trees**:

- `conformance/valid/minimal/describe.golden.json`
- `conformance/valid/minimal/validate.golden.json`
- `conformance/invalid/missing-basin-id-column/validate.golden.json`
- `conformance/invalid/basin-id-folder-mismatch/validate.golden.json`
- `conformance/invalid/ragged-field-schema/validate.golden.json`
- `conformance/invalid/non-monotonic-time/validate.golden.json`
- `conformance/invalid/missing-gridded-dynamic-subtree/validate.golden.json`

`regenerate.sh` -> `build.py` -> `mutate._copy_baseline` calls `shutil.rmtree`
on each invalid target before copytree, and the valid baseline is rewritten in
place. The generator has golden-awareness hacks to survive this:
`_copy_baseline(..., ignore=ignore_patterns("*.golden.json"))` and
`assertions._relative_files` excludes `*.golden.json`. Despite those hacks, a
fresh `rmtree`+regenerate of the invalid trees DELETES the tracked goldens that
live under them (the ignore only protects the baseline copy, not the pre-existing
target tree contents that `rmtree` wipes). **Goldens must move OUT of the two
gitignored trees** into a committed dir the generator never touches.

### What `validate.rs` actually runs vs skips (grounds the README matrix in S4)

`build_report` lists all 20 ids. On the **valid** fixture:

- **ran:pass** — M1, M2, M3, M4 (entry-gate convention), M5, L1, L2, I1, I2, I3,
  H1, H2, T1, G1, G2, G3, Geo1.
- **skipped (honest R3, never flips conformant)** — M6 (rule (b) regularity leg,
  `ByteDeep`), L3 (absence-vs-NaN leg, `ByteDeep`), T2 (cross-artifact full-axis
  identity, `ByteDeep`).

Enforcement notes that bound the new negatives:

- **M5** (`check_m5`, validate.rs:970) compares every `GridInfo.crs()` AND the
  outlines crs against `manifest.crs()`. A mismatch ⇒ `ran:fail`. The outlines
  leg is `ran:fail` only when its `crs_source == EpsgFromProjjsonId` (the fixture
  case: EPSG:4326); a `RawProjjsonR3` outlines leg skips. So a manifest-crs
  mutation trips M5 (via the grids leg, and the outlines leg too — both are the
  SAME check id M5, so it remains exactly-one-check-failing).
- **H2** (`check_h2`, validate.rs:561) compares each basin's static⊕dynamic
  grid-label set against the reference basin. A divergent label on one basin ⇒
  `ran:fail`.
- **G2** (`check_g2`, validate.rs:1113) only fires for a label present in BOTH a
  basin's COG and Zarr subtrees; it then requires extent==, resolution==,
  width==, height==. A shared-but-misaligned grid ⇒ `ran:fail`.

### Already done + committed (do NOT redo)

Generator + valid four-quadrant baseline; invalids with pinned regression tests +
committed validate goldens for: wrong-format-version (M2), missing-root-rollup
(L1) [MS2]; extra-manifest-field (M3), empty-cadence (M4) [entry-gate Err];
missing-basin-id-column (I1), basin-id-folder-mismatch (I2), ragged-field-schema
(H1), non-monotonic-time (T1), missing-gridded-dynamic-subtree (L2). describe +
validate goldens for valid/minimal. gitignore-fixture-data migration (v0.1.48)
and the MS8 decompose audit (v0.1.49). Current version: 0.1.49; latest tag
v0.1.49.

### Golden naming scheme (decided here, used by every step)

Relocate to a committed, generator-untouched dir:
`conformance/goldens/<fixture>.<verb>.json`, where `<fixture>` is the
slash-flattened fixture path with `/` → `-`:

| Old (in gitignored tree) | New (committed) |
|---|---|
| `valid/minimal/describe.golden.json` | `goldens/valid-minimal.describe.json` |
| `valid/minimal/validate.golden.json` | `goldens/valid-minimal.validate.json` |
| `invalid/<name>/validate.golden.json` | `goldens/invalid-<name>.validate.json` |

`conformance/goldens/` is a plain tracked dir (NOT under `valid/`/`invalid/`), so
no `.gitignore` rule touches it and `regenerate.sh` never rmtree's it.

---

## Step 1 — MS8C-S1: Relocate the 7 goldens out of the gitignored trees

(Detailed in the structured object; summary here.)

Move all 7 goldens to `conformance/goldens/`, repoint both Rust test helpers,
strip the now-unneeded golden-awareness from the generator, update `.gitignore`
and the README golden paths/workflow. After this step a fresh `regenerate.sh`
followed by `cargo test` is green with goldens intact — `regenerate.sh` no longer
clobbers them because they no longer live where it writes.

## Step 2 — MS8C-S2: M5 / G2 / H2 on-disk negatives

Three new invalid fixtures, each one surgical mutation off the baseline, each
pinning exactly one §14 check `ran:fail`, each with a relocated golden:

- `crs-mismatch` → pins **M5** (manifest crs mutated to EPSG:3857; files keep
  4326).
- `divergent-grid-label-set` → pins **H2** (one basin's COG+Zarr renamed
  era5 → era5b so its label set differs).
- `misaligned-shared-label` → pins **G2** (one basin's COG geometry shifted/scaled
  so the shared `era5` label is no longer cell-for-cell aligned with that basin's
  Zarr).

## Step 3 — MS8C-S3: M6 still-conformant irregular-time-axis fixture

A valid-shaped fixture with an irregular per-basin time axis; assert M6 is
`skipped`-with-reason and the dataset is STILL `conformant:true`. No enforceable
M6 negative exists in v0.1 (the regularity leg is R3-skipped). Relocated golden +
regression test + README/finding documentation.

## Step 4 — MS8C-S4: README check-id matrix + findings + architecture amendment

Complete `conformance/README.md` with the 20-check classification matrix, the
fixture→pinned-check map, confirmation against what `validate.rs` runs/skips, and
a dated `architecture.md` Amendments-log entry recording the
fixtures-gitignored + golden-relocation policy.

---

## Ordering rationale

Golden relocation (S1) is first and unblocks everything: until goldens live
outside the gitignored trees, every later step's `regenerate.sh; cargo test`
acceptance gate is unreliable (a regenerate could wipe a golden the step just
added). S2 adds the three remaining enforceable on-disk negatives (the meat). S3
adds the single documented non-enforceable M6 case (depends on the golden
location S1 settled, independent of S2). S4 is documentation/classification last,
because it must describe the complete fixture set S2/S3 produced and confirm it
against the frozen `validate.rs`.

## Scope guard

No `hdx-core` source change in any step (frozen for MS8): the readers, rule
functions, domain types, manifest floor, and the report/describe wire shapes are
untouched. No regrid/clip/reduce. No fixture DATA committed (the trees stay
gitignored) — only generator source, relocated `conformance/goldens/*.json`, the
README, the Rust tests, `.gitignore`, `architecture.md`, and `Cargo.toml`/lock.
Every invalid fixture stays exactly one surgical mutation off the valid baseline
(LOW-2) with the generator's `assert_differs_in_exactly_one_way` self-assertion;
goldens are produced by the Rust verb, never the Python generator. Every step:
`regenerate.sh` then `cargo build`+`test`+`clippy --all-targets -D warnings`
green, goldens survive regenerate, plus the mandatory patch bump + conventional
commit + tag.
