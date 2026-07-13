# HDX — Hydrology Dataset Exchange (format_version `0.2` current; `0.1` also recognized)

> **Status: canonical in-repo specification.** This document is the sole
> normative source of truth for the HDX format and is self-contained: every
> conformance requirement is stated here in full.
>
> Normative keywords **MUST**, **MUST NOT**, **SHOULD**, **MAY** are used in the
> RFC 2119 sense. A dataset is **conformant** iff it satisfies every **MUST** in
> this document for its declared `format_version`.

---

## 0. Reading order & versioning discipline

1. `format_version` **MUST** be readable before anything else in the dataset.
2. `format_version` is a **HARD version cut**: a reader **MUST** reject any
   version it does not implement. There are **no multi-version readers**. This
   spec defines `format_version = "0.1"` and `format_version = "0.2"`.
   - **`"0.2"` (geometry-optional).** `0.2` relaxes the geometry mandate:
     `outlines.geoparquet` becomes **OPTIONAL** (§4 L1, §9 Geo1, §3 I1), so a
     pure-scalar series (e.g. streamflow with no delineation) is conformant. The
     basin-set source-of-truth moves from `outlines.basin_id` to the still-mandatory
     `scalar_static.basin_id`; `describe.delineations` MAY be empty. Honors the
     hard-version-cut discipline: **a relaxation still bumps the version** — a `0.1`
     reader still rejects a `0.2` dataset, and a `0.2` reader admits both shapes.
     This is the first deliberate contract change since `0.1` froze; the engine
     mints `"0.2"` **wholesale** (every producer build, geometry-bearing or not).
3. HDX versions **the contract, not the content**. There is **no content hash**
   and **no data-version field** in HDX. (Resolved open question 5.)
   - "data version" (`v1`/`v2`) is the experiment workspace's label — not HDX's.
   - source vintage (ERA5 release, CAMELS edition) is provenance — not HDX's.
   - a content hash is derivable from the bytes, so it violates the manifest
     floor (§11) and is owned by the experiment layer, not HDX.

---

## 1. What HDX is — and is not

HDX is a prescriptive **data interface** for per-basin hydrology datasets. It
specifies *what the bytes look like and how they are organized* — nothing more.
It is cloud-optimized (its primary purpose is cloud training of deep-learning
models; §8) but MAY equally be hosted locally.

**The governing discipline (load-bearing, repeated everywhere):**

> **HDX describes the *shape* of data, never *what was done to it*.**

HDX is **inert** and **agnostic**. A conformant reader/writer **MUST NOT** carry,
require, or interpret any of the following in HDX itself:

- **transform / normalization state or fitted params** (μ/σ, log-ε, …). Data
  MAY sit in real space or transformed space; HDX records neither which nor the
  params. (These belong to an upstream preparation layer, outside HDX.)
- **roles** — which field is "target" / "forcing" / "future-known". That is a
  downstream modeling choice, not a property of the data.
- **semantic types** — continuous / categorical. Interpretation is the
  consumer's job.
- **the gridded → lumped reduction** as a fact in the data. The reduction is an
  *operation* on the data, not part of the contract (§10), never a manifest
  field.
- **provenance of computation** — what model/run/pipeline produced a dataset.
  HDX does not know whether a dataset is raw forcing or model output. **A
  prediction dataset is just an HDX dataset.**

Every time a design drifts toward encoding *what was done* or *what the data is
for*, it is wrong.

---

## 2. The spine — fields

The unit of HDX is the **field** (deliberately generic: a scientific variable, a
QC mask, a cluster id, and a model prediction are all *just fields*; HDX
privileges none). A field has **two independent axes**:

| Axis | Values | Meaning |
|---|---|---|
| **temporal** | `dynamic` \| `static` | a time series, or one value |
| **shape** | `gridded` \| `scalar` | a per-cell field, or a single value |

The shape axis is **`gridded` vs `scalar`**, *not* "gridded vs lumped". "Lumped"
smuggles in a reduction (an area-average); a scalar value (outlet streamflow at a
gauge) is often scalar **by nature**. HDX cares only about data shape — *do we
feed the model a grid or a single number* — not how a scalar came to exist.

A field also carries a **name**, **units** (or none), and a **dtype**.

The four quadrants, by per-basin array shape and physical encoding:

| Quadrant | Per-basin shape | Example | Physical encoding |
|---|---|---|---|
| `scalar · static`  | `[]`      | drainage area      | parquet column (dataset-level rollup, §4) |
| `scalar · dynamic` | `[T]`     | outlet streamflow  | parquet column (per-basin) |
| `gridded · static` | `[Y,X]`   | elevation raster   | COG band (per-basin) |
| `gridded · dynamic`| `[T,Y,X]` | precip over grid   | Zarr array / variable (per-basin) |

**The quadrant is a per-field classification, not a dataset-level mode.** A
single HDX dataset's field schema MAY contain fields from any combination of the
four quadrants — a dataset with both `gridded · dynamic` forcing and
`scalar · dynamic` streamflow, plus `scalar · static` attributes, is ordinary and
fully conformant. Homogeneity (§5) requires that the *set* of fields be identical
across basins; it does **not** require all fields to share a quadrant. A
conformant dataset carries whatever subset of the four physical encodings its
field schema implies (e.g. a scalar-only dataset has no `gridded_*` artifacts).

**Field name → column / variable / band is 1:1 and opaque.** A field name is a
unique producer-chosen string; the column / CF variable / band description is
named exactly that. HDX **parses nothing**: no canonical variable vocabulary, no
source/variable split. Multiple products (`ERA5_precipitation`,
`CHIRPS_precipitation`, …) are simply multiple fields; "one variable, three
sources" is a *downstream modeling* grouping, invisible to HDX.

**Companion masks** (`{field}_was_filled`, etc.) are **allowed and ordinary, but
not load-bearing**. HDX gives the suffix no magic and parses no "belongs-to"
link. Naming patterns (`{source}_{variable}`, `{field}_was_filled`) MAY be
*recommended as guidance* but a conformant reader/validator **MUST NOT** depend
on them.

---

## 3. Basin identity

- Each basin has an id **unique within the dataset**. Uniqueness is the only
  requirement; *how* it is minted (gauge id, hash, integer, UUID) is the
  producer's business.
- The id column everywhere is **`basin_id`** (never `gauge_id` — basins may be
  ungauged; never `group_identifier` — too vague).
- `basin_id` is the **authoritative in-file id**; the `basin=<id>` folder gives
  locality. A validator **MUST** cross-check that the in-file `basin_id` agrees
  with its `basin=<id>` partition folder.
- **Under `0.2`**, the `outlines` leg of the in-file `basin_id` check (I1) is
  **conditional** on geometry being present: `outlines.geoparquet` is OPTIONAL
  (§9), so its `basin_id` column is checked only when outlines ship. The
  `scalar_static` and `scalar_dynamic` `basin_id` legs stay **unconditionally**
  enforced, and `scalar_static.basin_id` is the basin-set source-of-truth.

---

## 4. On-disk layout — basin-first hive

HDX is the **hive-partition contract generalized by data shape**: the directory
structure *is* the contract; only the file format changes across the 2×2 (scalar
→ parquet, gridded → Zarr/COG). Partitioning is **basin-first** (natural access:
"give me everything for basin X").

```
<hdx-dataset>/
  manifest.json                       # the floor (§11)
  scalar_static.parquet               # dataset-level rollup; 1 row/basin; cols = basin_id + static scalar fields
  outlines.geoparquet                 # dataset-level; rows = (basin_id, delineation, geometry)
  basin=<id>/
    scalar_dynamic.parquet            # rows = time (real `time` coord); cols = basin_id + dynamic scalar fields
    gridded_static/<grid-label>.tif   # multiband COG; named bands = static gridded fields sharing this grid
    gridded_dynamic/<grid-label>.zarr # Zarr v3; named CF variables = dynamic gridded fields sharing this grid
  basin=<id>/ …
```

**Asymmetry (principled — tracks data size/shape, not convention):**

- Two **dataset-level rollups** sit at the root: `scalar_static.parquet` and
  `outlines.geoparquet`.
- Only the *large* per-basin data lives under `basin=<id>/`:
  `scalar_dynamic.parquet`, `gridded_static/`, `gridded_dynamic/`.

**Geometry-optional under `0.2`.** `scalar_static.parquet` is the dataset-level
floor and is **always required** (it carries the basin-set source-of-truth
`basin_id`, §3). Under `format_version = "0.2"`, `outlines.geoparquet` becomes
**OPTIONAL**: a geometry-less dataset (a pure-scalar series) omits it and is still
conformant. The L1 conformance check (§14) splits accordingly — `scalar_static` is
unconditional, the `outlines` leg is conditional on a **geometry-expected**
predicate carried by the schema. Under `0.1`, `outlines.geoparquet` remains
mandatory.

`scalar_static` rolls up to one table because static-scalar data *is* a
basins×attributes table, the access pattern is cross-basin (cohort/clustering),
the whole table is a few MB (loaded once, held in memory), and it dodges the
50k-tiny-files cloud anti-pattern. (Resolved open question 4.)

Each `basin=<id>/gridded_static/<label>.tif` **MUST** be a multiband TIFF with
`SamplesPerPixel = N`, where `N >= 1`. Its physical bands are TIFF samples
`0..N-1`, stored with `PlanarConfiguration = 2` (separate, band-sequential
sample planes). Physical sample `i` **MUST** self-name through a GDAL metadata
item written exactly as `<Item sample="i" role="description">FIELD</Item>`.
Units **MAY** be supplied by the analogous GDAL units item carrying the same
`sample="i"` index. All samples within one `<label>.tif` **MUST** use the same
TIFF dtype; this is per-label dtype homogeneity and does not require different
labels to share a dtype. The TIFF **MUST** carry one dataset-level `GDAL_NODATA`
value, which applies to every band; per-band nodata values are **NOT PERMITTED**.

---

## 5. Homogeneity

- Every basin in a dataset **MUST** have the **identical field schema**.
- A field absent for a given basin is **present-but-NaN**, **never** a missing
  file.
- Consequence and intent: **discovery is a one-basin read** — the point of hive
  partitioning. `describe` MAY read a single basin to enumerate fields.
- A set of basins that genuinely needs a different schema is a **different HDX
  dataset / data version**, not ragged coverage within one.

---

## 6. Time

1. **Per-basin, ragged time axes.** Basins differ in period of record;
   homogeneity is about *fields*, not *time extent*.
2. **One shared, aligned axis within a basin.** `scalar_dynamic.parquet` and
   every `gridded_dynamic` artifact in a basin **MUST** use the *same*
   timestamps. A field that does not natively cover the basin's full span is
   **NaN-filled** over the gap (the time-axis twin of §5). This lets a consumer
   align "forcing grid at *t*" with "target scalar at *t*" without resampling.
3. **A proper temporal type** — parquet `Date32`/`Timestamp`, Zarr CF
   integer-since-epoch — **MUST** be used. The caravan `String "YYYY-MM-DD"`
   hack is **forbidden**.
4. The dataset-wide **cadence/calendar** (e.g. `daily, proleptic_gregorian`) is
   a declared manifest convention (§11).

**Scalar parquet time column (resolved open question 1):**

- Named **`time`** (matching the gridded Zarr `time` dimension, so scalar and
  gridded line up on one same-named coordinate).
- A **full timestamp** (date+time), one uniform type for all datasets — the
  harmless superset that never forces a "daily or hourly?" branch and is
  future-proof for sub-daily.
- **Non-nullable**, **sorted ascending**.

---

## 7. Grids

1. **Gridded is dense rectangular** — `[Y,X]` / `[T,Y,X]` over the basin bbox.
   (Sparse cell-lists + coords are a *downstream* derivation, not a second HDX
   encoding.)
2. **Per-variable native grids.** Each gridded field keeps its **own**
   resolution / extent / affine — **no imposed common grid**. Heterogeneous
   grids are fine (ship ERA5, CHIRPS, MSWEP at true resolutions); each field's
   explicit cell coordinates make it independently usable. Regridding is a
   downstream op.
3. **Standard self-describing georeferencing.** Zarr **MUST** use CF conventions
   (explicit `lat`/`lon` coordinate arrays + `grid_mapping`/CRS); GeoTIFF
   **MUST** carry standard georeferencing tags. Units ride in the CF `units`
   attribute / TIFF band metadata.
4. **One dataset-wide CRS** (recommend **EPSG:4326**), declared in the manifest
   and carried in the files, so cells from different fields share one coordinate
   space even at different resolutions.

---

## 8. Delivery — cloud-optimized

HDX is **optimized for cloud hosting** and the random `(basin, time-window,
fields)` access of remote-GPU deep-learning training — the case the encoding
exists to serve. It **MAY also be hosted locally** (the same files work from a
local filesystem); the cloud-training case is simply the design driver. The
encoding rules:

- **Packing rule: one artifact = one grid.** Fields sharing an identical grid
  pack into one artifact. Fields on different grids stay in separate artifacts.
- **Self-naming physical fields.** Each dynamic gridded field is its own **named
  CF variable** in the Zarr (`[time,lat,lon]`). Each static gridded field is its
  own labelled physical TIFF sample in the COG, named by the exact indexed GDAL
  description item required in §4. Field names live in-file, so discovery stays
  a one-file read. Consumer channel order, when declared in §11, is a separate
  ABI and **MUST NOT** be inferred from physical TIFF sample order.
- **The artifact is named after its grid** — a stable, producer-chosen **grid
  label** (`gridded_dynamic/era5.zarr`, `gridded_static/era5.tif`). The label
  names the *grid family* (the literal per-basin extent/affine lives in-file). A
  **shared label across the `gridded_static` and `gridded_dynamic` subtrees
  signals cell-for-cell alignment** without opening either file.
- **gridded dynamic → Zarr v3:** time-major chunking sized to the lookback
  window (a `[t-n,t]` read is one contiguous range), **v3 sharding** (sane S3
  object counts at 50k basins), **consolidated metadata** (one GET to learn the
  store), blosc-zstd compression.
- **gridded static → multiband COG:** internal tiling + overviews.
- **scalar → parquet:** sorted by `time`, **row-group statistics written**
  (predicate pushdown / range reads), zstd.

---

## 9. Geometry — outlines ship, grids stay neutral

- **Geometry ships *in* HDX, as outline polygons in geoparquet**, and is
  **plural**: one row per delineation, each labeled by a neutral **`delineation`**
  value (MERIT / GRIT / HydroBASINS / a custom run / a hand-drawn polygon — *not*
  assumed to be a published "hydrofabric"). Disagreement between delineations is
  itself a modeling signal.
- **Outlines live in one dataset-level `outlines.geoparquet`** (resolved open
  question 2) — *not* per-basin, *not* partitioned by delineation. Rows are
  `(basin_id, delineation, geometry)`; a basin's competing delineations sit
  together, discernible by the `delineation` column.
- **Gridded fields ship delineation-neutral over the bbox** — *not* pre-clipped
  or NaN'd to any one outline.
- **Clipping / masking / area-weighting is a downstream operation** (trivial
  because of §7.3) — out of HDX scope.
- **Under `0.2`, outlines are OPTIONAL** (§0, §4). When `outlines.geoparquet` is
  absent the Geo1 check (§14) is **skipped** (None → Skipped), not failed; the
  dataset is a geometry-less pure-scalar series. When outlines DO ship, the full
  Geo1 shape (`(basin_id, delineation, geometry)`, `delineation`-labeled, not
  partitioned by delineation) is enforced unchanged. Outlines are load-bearing
  only for downstream clip/mask/area-reduce, which HDX does not own (§13).

---

## 10. Tooling — the contract-executing verbs

The split test: **does the verb *define/execute the contract*, or does it
*operate on data* the contract merely describes?**

**HDX owns ONLY the contract-executing verbs — and these two ARE the contract-executing verbs of HDX:**

- **`validate`** — conformance. The spec and its validator are the *same
  artifact*. Rust, in `hdx-core`; "parse, don't validate" (invariants in
  constructors). A dataset passes iff every **MUST** here holds.
- **`describe`** — discovery. Emits the full self-description discovered from the
  files: field catalog (the 2×2 quadrant per field), per-field grids, time
  ranges, units, delineation labels, basin list. `describe` is the **stress test
  of the manifest floor (§11)**: if `describe` is hard to implement, the floor is
  too thin.

Both are **the spec executed** and cannot drift from it because they *are* it. A
thin, JSON-emitting, LLM-drivable CLI wraps them (`hdx validate ./out`,
`hdx describe ./out`); PyO3 mirrors them.

**EXCLUDED from HDX:** `regrid`, `clip`, `reduce` — they encode hydrology
(area-weighting, partial-cell handling, resampling kernels), not the contract.
They *operate on* data HDX merely describes, so they belong to a separate
data-operations engine, **outside HDX**. Building these is **out of scope for
HDX** and they MUST NOT appear in `hdx-core`.

---

## 11. The manifest — non-derivable declarations

Hive + self-describing files + homogeneity make almost everything discoverable,
so the manifest contains six required floor fields and one optional known
consumer ABI declaration, all non-derivable. The six-field legacy form remains
valid:

```json
{
  "format_version": "0.2",
  "name": "<dataset name>",
  "created_at": "2026-06-01T00:00:00Z",
  "producer_version": "<tool/version that wrote it>",
  "crs": "EPSG:4326",
  "cadence": "daily"
}
```

The optional declaration expands the manifest as follows:

```json
{
  "format_version": "0.2",
  "name": "<dataset name>",
  "created_at": "2026-06-01T00:00:00Z",
  "producer_version": "<tool/version that wrote it>",
  "crs": "EPSG:4326",
  "cadence": "daily",
  "gridded_static_channels": {
    "era5": ["elevation", "soil_depth"],
    "landcover": ["forest_fraction", "urban_fraction"]
  }
}
```

These six required fields and one optional known property are the only manifest
properties. Field meanings and rules:

| Field | Rule |
|---|---|
| `format_version` | MUST be read first; **hard cut** (reject unknown). `"0.1"` or `"0.2"` (§0); `"0.2"` = geometry-optional. |
| `name` | dataset identity — generic, *not* "I am ensemble member 3". |
| `created_at` | RFC 3339 timestamp. |
| `producer_version` | the tool/version that wrote the dataset. |
| `crs` | dataset-wide CRS; MUST match the CRS carried in the files. |
| `cadence` | dataset-wide cadence/calendar; a validated convention (§6.4). |
| `gridded_static_channels` | OPTIONAL per-grid consumer channel ABI. It is an object whose keys are non-empty grid-label strings and whose values are non-empty ordered arrays of unique non-empty field-name strings. Object-member order is insignificant. Absence and `{}` both mean an empty legacy declaration. |

`gridded_static_channels` is non-derivable declared consumer ABI data. Its array
order is significant consumer order, independent of physical TIFF sample order.
Consumers bind each declared name to the self-named TIFF sample and **MUST NOT**
treat the array index as a physical sample index. For example, a TIFF physically
storing samples `[soil_depth, elevation]` may legally declare consumer order
`"era5": ["elevation", "soil_depth"]` because binding is by sample description.
This declaration defines and validates shape only; append-only evolution and
comparison with an earlier manifest are producer-side concerns outside this
contract step.

**Everything else** — catalogs, basins, grids, time ranges, units, delineation
labels, and other derivable facts — is **discovered**, never declared. Adding any
unknown or derivable property to the manifest is a **conformance bug**: a value
that restates the data can drift from it. Unknown-field rejection remains strict.

---

## 12. Annotations — just fields

HDX has **no annotation concept, no categorical type, no codebook, no auxiliary
mechanism.** Annotations decompose into ordinary fields:

- QC / gap masks → a boolean dynamic field.
- cluster id → an integer static field. *What `7` means is the consumer's
  problem, not HDX's.*
- regime / change-points → an integer/boolean dynamic field (`regime_id`,
  `is_break`).

The rich residue that does **not** decompose into a per-basin/per-timestep field
(centroid series, per-break statistics, basin×basin matrices) **is not HDX's** —
it stays in the producing tool's own output, outside HDX.

---

## 13. Scope boundary — what HDX carries and what it does not

The spec describes the **format**, nothing around it. This table draws the line;
the "in HDX" rows are the contract, the "not HDX" rows are the inert/agnostic
discipline (§1) made concrete. HDX introduces no conventions for how data is
*used*, *produced*, or *consumed*.

| Concern | In HDX? |
|---|---|
| field shapes, layout, georeferencing, time axis, homogeneity | **Yes — this is HDX** |
| conformance (`validate`) + discovery (`describe`) | **Yes — the spec executed** |
| transform / normalization params, fitted state | No — out of scope (§1) |
| field roles (target / forcing / future-known), split intent, cohort | No — out of scope (§1) |
| semantic types (continuous / categorical) | No — the consumer's job (§1) |
| reductions, regridding, clipping (operations on the data) | No — not the contract (§10) |
| provenance of computation; model / member / ensemble identity | No — HDX is agnostic (§1) |
| basin geometry *source* | No — outlines are copied in as neutral labeled data (§9); HDX never references or trusts the originating source |

Because HDX is agnostic to provenance (§1), it does **not** know or record
whether a dataset is raw input, an intermediate, or a model prediction — a
prediction dataset is just an HDX dataset, validated by the same rules. Nothing
in the format distinguishes them, and the format adds nothing for downstream
producers or consumers to exploit.

---

## 14. Conformance summary — the `MUST` checklist (validator scope)

A dataset is conformant iff all of the following hold. (This is the executable
floor `validate` enforces; it is derived from §1–§13 and introduces no new
requirements.)

**Manifest**
- M1 `manifest.json` exists, is valid JSON, and `format_version` is read first.
- M2 `format_version` is `"0.1"` or `"0.2"`; any other value is rejected outright
  (hard cut). `"0.2"` admits the geometry-optional shape (L1/Geo1/I1 below).
- M3 All six required floor fields are present, the optional known channel
  declaration conforms when present, and no unknown or derivable fields are
  present (§11).
- M4 `created_at` is RFC 3339; `crs`, `cadence` are non-empty strings.
- M5 `crs` matches the CRS carried in every georeferenced file.
- M6 `cadence` is consistent with the realized `time` axes.

**Layout**
- L1 `scalar_static.parquet` exists at the root (unconditional floor).
  `outlines.geoparquet` exists at the root **under `0.1`**; **under `0.2`** the
  outlines leg is conditional on the geometry-expected predicate (OPTIONAL when
  geometry-less, §4/§9).
- L2 Every basin directory matches `basin=<id>` and contains
  `scalar_dynamic.parquet` (and `gridded_static/` / `gridded_dynamic/` artifacts
  iff the schema declares gridded fields).
- L3 No stray/ragged files; absence of a field is NaN, never a missing file (§5).

**Identity**
- I1 `basin_id` is a real in-file column in `scalar_static` and every
  `scalar_dynamic` (unconditional); and in `outlines` when outlines ship (the
  outlines leg is conditional under `0.2`, §3). `scalar_static.basin_id` is the
  basin-set source-of-truth.
- I2 In-file `basin_id` agrees with the `basin=<id>` folder (§3).
- I3 `basin_id` is unique within the dataset.

**Homogeneity**
- H1 Every basin has the identical field schema (same field names, dtypes,
  quadrants).
- H2 Grid-label set is identical across basins.

**Time**
- T1 The scalar `time` column is named `time`, a full timestamp, non-nullable,
  sorted ascending.
- T2 Within each basin, `scalar_dynamic` and all `gridded_dynamic` artifacts
  share the identical time axis (§6.2); gaps are NaN-filled.

**Grids / artifacts**
- G1 One artifact = one grid; fields self-name (CF variable / indexed COG sample
  description = field name). A declared static consumer channel order binds by
  those descriptions and is independent of physical TIFF sample order.
- G2 Each artifact is named after its grid label; a shared label across the
  static/dynamic subtrees implies (and MUST exhibit) cell-for-cell alignment.
- G3 Zarr is CF-georeferenced (explicit `lat`/`lon` + `grid_mapping`). Each COG
  carries standard georeferencing tags and satisfies the §4 multiband contract:
  `SamplesPerPixel >= 1`, physical samples `0..N-1`,
  `PlanarConfiguration = 2`, exact indexed GDAL descriptions, one dtype per
  label, and one dataset-level `GDAL_NODATA` applying to every band with no
  per-band nodata.

**Geometry**
- Geo1 `outlines.geoparquet` has rows `(basin_id, delineation, geometry)`; the
  label column is `delineation`; not partitioned by delineation. **Under `0.2`,
  when outlines are absent this check is skipped** (None → Skipped, §9); when they
  ship it is enforced unchanged.

> **Note on enforcement depth.** Some checks (e.g. byte-level Zarr/COG internals,
> full sharding/overview verification) MAY be implemented incrementally; the
> validator MUST clearly report which checks ran. The `MUST` list above is the
> conformance target; the milestone plan sequences how much is enforced when.
