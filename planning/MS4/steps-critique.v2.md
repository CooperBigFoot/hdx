# MS4 STEP-PLAN CRITIQUE (adversarial, ground-truth-verified)

**Milestone:** MS4 — Gridded + geometry metadata readers (discovery layer, gridded/geometry half)
**Verdict:** NOT APPROVED (highest severity: MEDIUM). Zero high/critical issues; two
MEDIUM gaps (an unpinned load-bearing dependency and a deliverable/coverage
mismatch) plus two LOW items must be resolved before this plan is execution-ready.

---

## Method — what was verified against the actual repo (not taken on faith)

Every load-bearing ground-truth claim in the plan was checked against the committed
MS2 fixture, the installed crate sources, and the current `crates/core` code:

- **Zarr (MED-5).** Read `basin=0001/gridded_dynamic/era5.zarr/zarr.json` directly:
  it carries inline `consolidated_metadata` (`"kind": "inline"`) holding `crs`,
  `era5_precipitation`, `era5_precipitation_was_filled`, `lat`, `lon`, `time`. Plain
  JSON (`serde_json`-readable). Data arrays carry `dimension_names: ["time","lat","lon"]`;
  the int8 mask has `data_type: "int8"`; the `crs` member's attributes hold
  `grid_mapping_name`, `crs_wkt`, and `spatial_ref: "EPSG:4326"`. Data arrays use
  `sharding_indexed` (blosc/zstd); `lat`/`lon`/`time` use `bytes`+`zstd`. **All true.**
- **Zarr chunk layout (LOW-3).** Confirmed `era5_precipitation/c/0/0/0` and
  `era5_precipitation_was_filled/c/0/0/0` data chunks exist, while coord arrays have
  `lat/c/0`, `lon/c/0`, `time/c/0`. The no-data-chunk test (delete data `c/**`, keep
  coord `c/0`) is structurally feasible. **True.**
- **COG (MED-4).** Parsed the TIFF IFD of `gridded_static/era5.tif` byte-by-byte:
  tag 42112 (GDAL_METADATA) = `<Item name="DESCRIPTION" sample="0" role="description">elevation</Item>`
  + `<Item name="units" sample="0">m</Item>`; tag 270 ABSENT; SampleFormat(339)=3,
  BitsPerSample(258)=32; ModelPixelScale(33550)=[0.25,0.25,0]; ModelTiePoint(33922)=
  [0,0,0,10,50,0]; GeoKeyDir(34735) carries EPSG 4326 (key 2048=4326); ImageWidth=6,
  ImageLength=8; Compression=deflate. **All true.**
- **tiff 0.11.3.** Present in the cargo cache, pure Rust. Verified the decoder exposes
  `get_tag_ascii_string`, `get_tag_u32_vec`, `get_tag_u16_vec`, `get_tag_f64_vec`,
  `Tag::Unknown`, and `dimensions()`. `Decoder::new` calls `next_image()` (reads the
  IFD/tags + chunk offset/bytecount tags) but does **not** decode pixel payload unless
  `read_image()` is called — so the LOW-3 "tags only, no pixels" claim is sound. **True.**
- **Geoparquet.** Columns exactly `(basin_id, delineation, geometry)`; `geometry` is
  arrow `binary` (geoarrow.wkb); `delineation` = `[merit, merit, merit, grit]`;
  `basin_id` = `[0001,0002,0003,0001]`; single root file. **All true.**
- **Crate state.** `CoreError` currently has 15 variants; `lib.rs` asserts
  `variants.len() == 15`. 15 + 4 (`ZarrRead`, `CogRead`, `GeoparquetRead`,
  `MissingOutlinesColumn`) = **19** — S1's arithmetic is correct. `Dtype` is the closed
  6-arm enum the plan extends. MS3 seams exist exactly as S5 assumes: `BasinDir::
  gridded_static()`/`gridded_dynamic()` (path + presence), `RootRollupPresence::
  outlines_present()`, `LayoutModel::outlines()`. The scalar reader's `ProjectionMask` /
  `ParquetRecordBatchReaderBuilder` bounded-1-D-column pattern (which S4 reuses) is real.
  README has the Dtype glossary line (`f32…timestamp`) and a Mermaid module map S5 edits.
  Baseline `cargo build -p hdx-core` is green. **All true.**

The plan's "ground-truth verification" section is accurate. The three folds are genuine.

---

## Folded STEP-2 issues — verification

- **MED-4 (COG band-description decision).** FOLDED, substantive. S3 records the
  explicit three-outcome decision in the architecture Amendments log with outcome (1)
  named (pure-Rust `tiff 0.11`, no GDAL), round-trips the band description on the
  fixture (`elevation`, units `m`), states "never silently reintroduce GDAL," and names
  the MS2-regenerate contingency. The chosen reader provably reads the tag the generator
  wrote (verified: tag 42112 holds DESCRIPTION=elevation). Not cosmetic.
- **MED-5 (Zarr consolidated-metadata, Rust-side).** FOLDED, substantive. S2 confirms
  from the Rust side via a dedicated test that the store is learned from the inline
  `consolidated_metadata` in the root `zarr.json` (one read), names the R3
  skip-with-reason fallback as the contingency, and states a mismatch is fixed by
  regenerating the fixture, never a reader workaround. Verified the fixture genuinely
  carries inline consolidated metadata.
- **LOW-3 (no-gridded-chunk / no-pixel gate).** FOLDED, substantive and mandatory. The
  scope guard + S2 (no-data-chunk test) + S3 (no-pixel test) are explicit and
  non-hedged; "where feasible" is correctly reserved for S5's roll-up only. The
  gridded_* subtrees are opaque leaves to the walk and metadata-only to the readers.

All three folds pass.

---

## ISSUES

### MED-1 (S2) — load-bearing zstd-decoder dependency left unpinned; "pure-Rust" framing at risk
**Category:** convention / vague-acceptance.
The GridExtent (`west/north/x_res/y_res`) is provably derivable ONLY by decoding the
`lat`/`lon` coordinate `c/0` chunks: the `lat`/`lon` member attributes carry only
`units`/`standard_name`/`axis` — NO extent bounds (verified). So a zstd decode of the
coordinate chunks is unavoidable for S2's stated extent assertions (west=10.0,
north=50.0, res=0.25). Yet S2's Cargo.toml change says only "no `zarrs`" and defers the
decoder choice — "the `zstd` crate already in the tree via parquet's `zstd` feature, or
a minimal `ruzstd` ... decide at implementation; pure-Rust only, no GDAL/C deps."

This is wrong on two counts:
1. `zstd 0.13` (the crate transitively pulled by `parquet`) wraps `zstd-sys` — it
   **bundles and compiles C**, it is NOT pure Rust. Depending on it directly
   contradicts the plan's own repeated "pure-Rust, no C deps" framing. Only `ruzstd`
   is pure Rust.
2. You cannot `use` a transitive dependency; S2 must add a **direct** dependency to
   `crates/core/Cargo.toml`. MS3 pinned its reader stack to exact majors with a
   feature-minimal rationale (arch amendment). S2 must do the same for the coord
   decoder — name the crate, the version, the feature set — not defer it.

**Suggested fix:** Pin `ruzstd` (pure Rust) at an exact major as a direct dependency in
S2's Cargo.toml change, with the same "metadata + 1-D coord reads only" rationale, and
record it in the R1-Zarr architecture amendment. If `zstd` (C) is knowingly accepted
because it is already in the tree, say so explicitly and drop the "no C deps" wording
for the Zarr coord path — but do not leave it as "decide at implementation."

### MED-2 (S2) — Zarr `time` coordinate-value read: deliverable + coverage-table over-claim
**Category:** missing-coverage / spec-drift (internal inconsistency).
The MS4 milestone deliverable says the Zarr reader reads "`time`/`lat`/`lon` coordinate
arrays," and the plan's own coverage table (line 652: "lat/lon/time coords") and S2 dep
note (line 318: "(`lat`/`lon`/`time`) ... decode with a pure-Rust zstd decoder") claim
all three coordinate arrays are decoded. But S2's actual `GridInfo` assembly (line
344–346) decodes only the `lat` and `lon` coordinate **values**; the `time` coordinate
**values** are never decoded, and `GridExtent { west, north, x_res, y_res, width,
height }` has no time dimension. The `time` member is read at the metadata level only
(shape/dtype/units from the consolidated `zarr.json`).

This is not fatal to MS4's *exit criteria* (G1/G3/G2-precond/Geo1/I1 need no time
values; T2 alignment is MS6; `describe`'s per-basin time extents can come from MS3's
parquet `time` read under §6.2 intra-basin alignment). But the plan's tables assert a
coverage that S2 does not deliver, which is exactly the kind of drift this review must
flag. As written a reviewer cannot tell whether Zarr-time-value decode is intended,
deferred, or forgotten.

**Suggested fix:** Make the decision explicit. Either (a) add a one-line statement in
S2 that the Zarr `time` coordinate **values** are intentionally NOT decoded in MS4
(only its metadata), with the rationale that the time axis (extent for MS5, T2 for MS6)
is sourced from the parquet `time` column under §6.2 — and correct the coverage table
(line 652) and the Cargo/dep note (line 318) to say lat/lon values + time *metadata*;
or (b) include the Zarr `time` chunk decode in S2 and surface a Zarr time axis. Do not
leave the tables claiming "time coords" while S2 reads only lat/lon.

### LOW-1 (S1) — green-ness depends on shell-accessor tests exercising every field/getter
**Category:** not-green (precondition).
S1 declares `GriddedDiscovery`/`GeometryDiscovery` shells with private fields + getters
"populated in S2–S5." The repo uses **no** `#[allow(dead_code)]` anywhere; its
discipline is to keep every declared item exercised by a test (cf.
`every_core_error_variant_constructs`, which constructs even the reserved skeleton
variants). With `clippy -- -D warnings`, S1 stays green ONLY if its unit tests construct
each shell and call every getter (and any constructor). The plan does state the
shell-accessor tests, so this is *covered*, but the dependency is implicit and easy to
under-deliver.

**Suggested fix:** State in S1's acceptance that every new struct field/getter/constructor
introduced ahead of its consumer is exercised by an S1 unit test (no `#[allow(dead_code)]`),
mirroring the existing error-variant-construction test, so S1 is clippy-green standalone.

### LOW-2 (S3/S5) — COG `.tif` file discovery from the subtree path not stated
**Category:** vague-acceptance.
`BasinDir::gridded_static()` returns the `gridded_static/` **subtree directory** path
(verified), not the `<label>.tif`. S3's `read_cog(path)` needs the `.tif` path and
derives `grid_label` from "the `.tif` filename." The plan implies but never states the
subtree-enumeration step that finds `<label>.tif` inside the subtree (and the analogous
`<label>.zarr` for S2). On the minimal fixture there is exactly one file per subtree, so
this is low risk, but the step that resolves "subtree dir → artifact file + label" is
unspecified.

**Suggested fix:** State in S3 (and S2/S5) that the reader enumerates the
`gridded_static/`/`gridded_dynamic/` subtree to locate the single `<label>.tif`/
`<label>.zarr`, deriving `grid_label` from that filename, and surfaces a typed
`CogRead`/`ZarrRead` if the subtree does not contain exactly one such artifact.

---

## What is correct (so it is not re-litigated)

- **Ordering** is dependency-correct: S1 (types, no IO) → S2/S3/S4 (mutually
  independent readers, each green against the real fixture) → S5 (seam consuming all
  three). Highest-risk reader (Zarr/MED-5) first. Buildable as written.
- **Scope/inert discipline.** No regrid/clip/reduce; no transform/role/semantic/
  provenance field; the six-field Manifest is untouched; CRS read verbatim (M5
  cross-check correctly deferred to MS6); the XML `role="description"` attribute is used
  only to locate the band name, not interpreted as an HDX role. GridInfo/GridExtent are
  purely structural (arch §3.5).
- **HIGH from iteration-1 (int8 vs closed Dtype)** is fixed at the root in S1 (`Dtype::I8`
  + parse/as_str/doc-table + round-trip tests), making S2 a mechanical application;
  typed-error-first preserved for genuinely unmapped dtypes (`uint8`/`int16` →
  `UnknownDtype`).
- **Error surface.** Four new inert/agnostic named-field variants with WHEN-doc-comments,
  `#[non_exhaustive]` retained, count literal 15→19, no MS1–MS3 variant reshaped. The
  distinct `GeoparquetRead` (vs `ParquetRead`) is a decided, defensible choice.
- **Conventions.** `tracing` only, `#[instrument]` on public reader fns, thiserror
  named fields, no unwrap/expect/panic in lib, explicit grouped imports, bump+tag per
  step, conventional commit messages. `clippy --all-targets -- -D warnings` (stricter
  than the milestone's bare `clippy -- -D warnings`) is fine.
- **Acceptance** ids are concrete (build/test/clippy + specific spec-check ids G1/G3/
  G2-precond/Geo1/I1 + named fixtures + exact extent/dtype/label assertions), not vague,
  except where flagged in MED-1/MED-2/LOW-1/LOW-2.

---

## Bottom line

The plan is technically sound and its ground-truth is real — every load-bearing
on-disk and crate claim was verified against the actual bytes/sources. The three
STEP-2 folds (MED-4, MED-5, LOW-3) are genuinely incorporated. There are **zero
high/critical issues.** But two MEDIUM items block a clean approval: (MED-1) the
load-bearing coordinate-decode dependency is left unpinned and clashes with the
plan's own "pure-Rust, no C deps" framing, and (MED-2) the plan's deliverable and
coverage tables claim the Zarr reader reads the `time` coordinate while S2 actually
reads only lat/lon — an unstated deferral that must be made explicit and the tables
corrected. Resolve MED-1, MED-2 (and ideally LOW-1, LOW-2) and the plan is
execution-ready.
