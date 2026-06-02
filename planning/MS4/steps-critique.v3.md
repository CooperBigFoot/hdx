# MS4 STEP-plan critique (adversarial review, iteration 3)

**Verdict: NOT APPROVED.** One **high-severity** correctness/spec-drift defect makes the
S2 and S5 acceptance criteria unsatisfiable against the committed fixture bytes and
breaks the G2 alignment-precondition observation that S5 is built to produce. Plus
one medium and a few low notes. The three folded STEP-2 issues (MED-4, MED-5, LOW-3)
are genuinely and substantively incorporated — those are clean.

All findings below were ground-truthed by decoding the real fixture bytes under
`conformance/valid/minimal/` and reading the existing `crates/core/src`.

---

## Ground truth established (by reading the fixture + repo)

| Claim in plan | Verified? | Evidence |
|---|---|---|
| Zarr store-root group `zarr.json` carries inline `consolidated_metadata` (§8 one-read path) | **TRUE** | top-level keys include `consolidated_metadata`; members `{crs, era5_precipitation, era5_precipitation_was_filled, lat, lon, time}` all inline |
| `era5_precipitation_was_filled` is `int8`; `Dtype` has no `I8` | **TRUE** | array `data_type: int8`; `field.rs` enum is `F32,F64,I32,I64,Bool,Timestamp`; `parse_dtype` has no int8 alias |
| coord chunks (`lat/lon/time` `c/0`) are zstd-framed (`28 b5 2f fd`) | **TRUE** | magic bytes confirmed; codecs `[bytes(little), zstd]`; the only in-tree zstd is `zstd 0.13` (zstd-sys/C), so a direct pure-Rust decoder is indeed required |
| COG tag 42112 carries `DESCRIPTION="elevation"`, `units="m"`; not in ImageDescription(270) | **TRUE** | 42112 GDAL XML: `<Item name="DESCRIPTION" sample="0" role="description">elevation</Item>`, `<Item name="units" sample="0">m</Item>`; tag 270 absent |
| COG georef tags 33550 / 33922 / 34735 present; f32; AdobeDeflate | **TRUE** | 33550=[0.25,0.25,0], 33922=[0,0,0,10.0,50.0,0], 34735 EPSG 4326; SampleFormat=3, BitsPerSample=32, Compression=8 |
| geoparquet schema `(basin_id, delineation, geometry)`, 4 rows, ≥2 delineations for a basin | **TRUE** | schema confirmed; rows: basin 0001 has both `merit` and `grit`; CRS in `geo` KV |
| `lib.rs` asserts `variants.len() == 15`; S1 bumps to 19 | **TRUE** | exactly 15 variants today; S1 adds 4 and updates the assertion in lockstep (correct — otherwise red) |
| MS3 seam `BasinDir::gridded_static()/gridded_dynamic()` + `RootRollupPresence::outlines_present()` exists | **TRUE** | both methods present in `layout.rs`/`discovery.rs` |

---

## HIGH — cell-center vs cell-edge: S2 extent formula contradicts the fixture; S5 "extents coincide" is false (correctness + spec-drift)

**This is the blocking defect.** It propagates S2 → S5.

The decoded Zarr 1-D coordinate arrays store **cell centers**:

```
lat = [49.875, 49.625, 49.375, 49.125, 48.875, 48.625, 48.375, 48.125]   (shape 8)
lon = [10.125, 10.375, 10.625, 10.875, 11.125, 11.375]                    (shape 6)
=> lat[0] = 49.875, lon[0] = 10.125
```

The COG `ModelTiepoint` (33922) stores **cell edges** (GeoTIFF PixelIsArea top-left):

```
ModelTiepoint = [0,0,0, 10.0, 50.0, 0]  =>  west(edge)=10.0, north(edge)=50.0
ImageWidth=6, ImageLength=8, PixelScale=0.25  (same 6x8 grid as the Zarr)
```

They describe the **same grid**: `Zarr center = COG edge + 0.5*pixel` (10.0 + 0.125 = 10.125; 50.0 - 0.125 = 49.875).

Now compare to the plan:

1. **S2 derivation (lines 326-327)** sets `GridExtent { west = lon[0], north = lat[0], ... }` — i.e. `west = 10.125, north = 49.875` (centers, no half-pixel shift).
2. **S2 acceptance (lines 347-348)** then asserts `west == 10.0`, `north == 50.0`. **This is arithmetically impossible** given the formula it just specified — `lon[0]` is `10.125`, not `10.0`. The step as written cannot go green; either the formula is wrong or the assertion is wrong, and they contradict each other.
3. **S3 acceptance (lines 430-431)** asserts COG `west == 10.0, north == 50.0` — **correct for the COG** (it reads the tiepoint edge directly).
4. **S5 (lines 567, 573)** asserts the Zarr `GridInfo` and the COG `GridInfo` extents **"coincide"** (and frames this as the on-disk G2 cell-for-cell-alignment precondition). With S2 producing center-based `10.125/49.875` and S3 producing edge-based `10.0/50.0`, **the two extents do NOT coincide**, so the S5 end-to-end assertion and the entire G2-precondition observation S5 exists to deliver are false as specified.

The plan never decides a single extent convention (both-edges or both-centers) nor specifies the half-pixel conversion that would reconcile a center-based Zarr coordinate array with an edge-based GeoTIFF tiepoint. This is exactly the kind of silent cross-format mismatch the milestone's "shared grid label ⇒ cell-for-cell alignment" precondition is supposed to surface honestly — and here the plan would either fail to compile its own asserts (S2) or assert a coincidence that is byte-for-byte false (S5).

**Why this is spec-relevant, not just a test bug:** spec §8/§14 G2 says a shared label "implies (and MUST exhibit) cell-for-cell alignment." The MS4 deliverable is to *observe* that precondition. An extent model that stores raw `lon[0]` for Zarr and raw tiepoint for COG would make two genuinely-aligned artifacts *look misaligned*, manufacturing a false G2 signal for MS6 to enforce on. That is a structural misreading of the grid, i.e. a correctness defect in the discovery layer.

**Suggested fix (pick one, state it, and assert it on the fixture):**
- Define `GridExtent` on a **single, explicit convention** — recommend **cell-edge origin** (the GeoTIFF-native convention): COG uses the tiepoint directly; the Zarr reader converts center→edge as `west = lon[0] - x_res/2`, `north = lat[0] + y_res/2` (sign per axis direction). Then both yield `10.0 / 50.0` and S5's "coincide" is true. Document the convention in the `GridExtent` doc and the S1 amendment.
- Whichever convention is chosen, **fix S2's formula and S2's acceptance to agree with each other and with the decoded bytes** (currently they do not), and make S5's "extents coincide" assertion actually hold for the chosen convention.
- Add an explicit S2 test asserting the *decoded* `lon[0]==10.125`/`lat[0]==49.875` (the raw coordinate truth) separately from the *derived* extent, so the half-pixel step is visible and locked.

---

## MEDIUM — S4 records CRS "verbatim" but the geoparquet CRS is a PROJJSON object, not `"EPSG:4326"`

S4 (lines 481-483, 493-494) says it reads `crs: Option<Crs>` **"verbatim from the parquet `geo` KV metadata `primary_column` CRS PROJJSON"**. Ground truth: the `geo` KV stores the geometry column CRS as a **PROJJSON object**: `{"id": {"authority": "EPSG", "code": 4326}, ...}` — **not** the string `"EPSG:4326"`.

`Crs` is a `Crs(String)` newtype. "Verbatim" is ambiguous here:
- If it stores the raw PROJJSON blob, then the recorded geoparquet CRS is a JSON object string, while the manifest CRS (and COG/Zarr CRS) is `"EPSG:4326"`. The MS6 spec-check M5 cross-check (which the plan repeatedly says it is *seeding*) would then be comparing `{"authority":"EPSG","code":4326}` to `"EPSG:4326"`.
- The milestone's own MS4 risk note (milestones.md lines 492-494) explicitly calls for **normalizing CRS strings consistently** across manifest/Zarr/COG/geoparquet so the MS6 M5 cross-check works. The plan's blanket "record verbatim, never normalize" stance (scope-guard line 203; S4 line 494) is in tension with that, specifically for the PROJJSON case.

This is not fatal for MS4 (no M5 *rule* runs here), but the plan should state **what `Crs` actually holds for the geoparquet path** (raw PROJJSON vs an extracted `EPSG:<code>`), so MS6 isn't handed an un-cross-checkable value. The Zarr (`spatial_ref="EPSG:4326"`) and COG (`EPSG:<code>` from GeoKey) paths already yield `EPSG:4326`-shaped strings; only geoparquet is the odd one out, and the plan glosses it as "verbatim."

**Suggested fix:** In S4, specify the geoparquet CRS extraction precisely — e.g. "read the PROJJSON `id` → `EPSG:<code>` string, recorded as `Crs`; if `id` is absent, record the raw PROJJSON and classify M5-readiness for this file as an R3 note." State the chosen normalization in the S4 acceptance and a test (`crs == EPSG:4326`).

---

## LOW — `tiff 0.11` / `ruzstd 0.7` APIs asserted "verified" but unverifiable offline; MED-4 covers only the band-description path

The plan asserts as fact: `tiff 0.11.3` exposes `Tag::Unknown(42112)` + `get_tag_ascii_string` and reads IFDs without decoding pixels (S3, line 65/404); `ruzstd = "0.7"` is the pinned pure-Rust decoder (S2, line 308). These crate APIs/versions could not be verified in this environment (no network). The **MED-4 three-outcome protocol fully de-risks the COG band-description read** (the highest-uncertainty item) — good. But there is **no analogous recorded fallback** if:
- `tiff 0.11`'s *georef-tag* read (33550/33922/34735) or its no-pixel guarantee differs, or
- `ruzstd 0.7`'s decode API differs (it has churned across recent majors).

These are lower-uncertainty than the band read, but the plan presents them as settled fact. Recommend a one-line contingency in S2/S3 acceptance: "if the pinned crate's API differs at implementation time, pin the working adjacent major and record it as an amendment" — so a version surprise is a recorded pin-bump, not an ad-hoc scramble that risks a red commit.

---

## LOW — prose/API-name slips (non-blocking)

- The plan's narrative writes the layout seam as `RootRollupPresence::outlines_present()` (header line 26, S5 line 524). Verified: `outlines_present()` *does* exist on `RootRollupPresence` (`discovery.rs`), so this is **correct** — noted only to confirm it is not a phantom API.
- S2 classifies the `crs` array "by `_ARRAY_DIMENSIONS`/`dimension_names` self-reference" alongside `lat/lon/time` (line 320), but the `crs` array has `dimension_names: null` and shape `[]`. The plan's *other* sentence (line 324: resolve it via a variable's `grid_mapping: "crs"`) is the correct mechanism and matches the fixture. The first sentence's grouping is loose wording, not a logic error — tighten it so the implementer doesn't try to classify `crs` as a coordinate by self-reference (it has no dimensions).

---

## Folded STEP-2 issues — verification (all genuinely incorporated)

- **MED-4 (COG band-description three-outcome decision): INCORPORATED, substantive.** Dedicated protocol section (lines 74-98) with the three named outcomes, the "never silently reintroduce GDAL" rule, the round-trip-on-fixture rule, and the "mismatch ⇒ MS2 regenerate, never a reader workaround" rule. S1 records the frame in the architecture Amendments log; S3 executes it against the real fixture and records the realized outcome (lines 419-420, 444-447). Ground truth confirms outcome (1) is achievable (tag 42112 is present and parseable; description="elevation", units="m"), so the expected "metadata-deep + live" path is real. **Not cosmetic.**
- **MED-5 (Zarr consolidated-metadata, Rust-side confirmation): INCORPORATED, substantive.** Dedicated protocol section (lines 100-114) with the live-vs-R3-skip outcomes and the "mismatch ⇒ regenerate the fixture, never a reader workaround" rule. S2 makes it a Rust test asserting all six members are discoverable from the single store-root `zarr.json` read (lines 336-339, 365-367). Ground truth confirms the consolidated block is physically inline with all six members, so the live path is real. **Not cosmetic.**
- **LOW-3 (no gridded-chunk decode / no pixel raster): INCORPORATED, substantive AND asserted.** Dedicated review-gate section (lines 116-133); restated in S2/S3/S5 acceptance; and — crucially — backed by *executable* tests, not just prose: S2 deletes the `era5_precipitation*/c/...` shard files from a temp copy and asserts the `GridInfo` is byte-identical (proving data chunks don't participate); S3 asserts facts come from the IFD with `read_image*` never called; S5 reuses the S2 temp-store test through the combined path. Ground truth confirms feasibility: data lives at `era5_precipitation/c/0/0/0` (deletable), coord chunks at `lat/c/0` (kept). **Not cosmetic.**

---

## Scope / conventions / ordering / green — assessment

- **Scope (clean).** No step adds `regrid`/`clip`/`reduce`, no transform/role/semantic/provenance field, no manifest field. `GridInfo`/`GridExtent`/`OutlinesInfo`/`GriddedGeometryDiscovery` are structural-fact-only. All §14 enforcement is correctly deferred to MS6; `describe` to MS5. The Zarr `time` *values* are intentionally not decoded (MED-2) and no MS4 exit criterion needs them — defensible recorded scoping, **not** a coverage gap (architecture §1 lists the 1-D time array as the source for the *MS6/T2* alignment concern, not an MS4 need).
- **Conventions (clean as planned).** New errors are named-field `thiserror` variants with when-it-fires docs, all inert. Readers raise typed `CoreError`, no `unwrap`/`expect`/`panic`/`println` in lib code, mirroring the established `scalar_reader.rs` discipline. New deps are direct + pinned-major + rationale-commented, pure-Rust, no GDAL — consistent with the MS3-S1 amendment style. `Dtype::I8` is the correct root fix (closed enum + boundary alias), not a reader hack.
- **Ordering (sound).** S1 (types/errors/amendment frame) → S2 (Zarr, produces the reference `GridInfo`/`GridExtent` shapes) → S3 (COG, reuses them) → S4 (geoparquet, independent) → S5 (assembler). No step depends on a later step. Each is independently committable. **Caveat:** S2 and S5 are not actually green-able *as written* because of the HIGH cell-center/edge defect — once that is fixed, the ordering holds.
- **Green/committable.** S1 correctly bumps the error-count assertion 15→19 in lockstep (otherwise red). The only obstacle to "each step green" is the HIGH defect (S2's self-contradictory extent assertion, S5's false coincidence assertion).
- **Acceptance quality.** Criteria are concrete (build/test/clippy + specific spec-check ids + named fixture assertions) and commit messages are conventional. The one exception is the HIGH defect, where S2's acceptance numbers contradict S2's own formula.

---

## What must change before approval

1. **(HIGH)** Resolve cell-center vs cell-edge: choose one `GridExtent` convention, specify the Zarr center→edge half-pixel conversion (or COG edge→center), fix S2's formula+acceptance so they agree with the decoded bytes (`lon[0]=10.125`, `lat[0]=49.875`), and make S5's "extents coincide" assertion actually hold. Add an S2 test on the raw decoded coordinates.
2. **(MED)** Specify exactly what `Crs` holds on the geoparquet path (PROJJSON `id` → `EPSG:<code>` vs raw blob) and assert it, so MS6's M5 cross-check is fed a comparable value.
3. **(LOW)** Add a one-line crate-API contingency for `tiff`/`ruzstd` version surprises (record a pin-bump as an amendment rather than risk a red commit).
4. **(LOW)** Tighten the S2 `crs`-array classification wording (resolve via `grid_mapping` target, not by dimension self-reference).
