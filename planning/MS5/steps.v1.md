# MS5 — `describe`: assemble + emit the full self-description (+ describe JSON Schema)

> **Milestone scope (verbatim intent, milestones.md MS5).** Implement `describe`
> (spec §10): from the **completed MS3+MS4 discovery layer**, assemble the
> `Description` (manifest, basins, homogeneous field catalog with the 2×2 quadrant
> per field, per-grid info, per-basin ragged time extents, units, delineation
> labels) and serialize it to **stable JSON**. `describe` **reports facts only — no
> conformance verdict** — but it still performs the **§0 hard version cut and
> manifest boundary-parse FIRST**, before any discovery. Pin the `describe` output
> JSON Schema in `schemas/` (R4). This is the spec's declared **stress test of the
> manifest floor** (§10/§11).
>
> **Hard boundaries (do not cross).** No `regrid` / `clip` / `reduce`, ever. No
> inert-violating field anywhere (no transform / role / semantic / provenance, no
> derivable manifest field). The manifest stays **exactly the six floor fields**;
> `format_version` is a **hard cut**. `describe` does **not** emit a conformance
> verdict and does **not** implement any §14 rule — that is MS6. `describe` only
> *reports* the discovery layer; it must **not reshape** `Discovery` /
> `ScalarDiscovery` / `GriddedDiscovery` (those are MS3/MS4 contracts). No
> gridded-chunk / pixel read enters anywhere (LOW-3) — `describe` reuses the
> metadata-only readers untouched.

---

## Ground truth (verified against the committed code + MS2 fixture before planning)

MS5 assembles `Description` from the **already-built** discovery layer; it adds no
reader. The shapes it consumes are fixed by MS3/MS4 and were read at plan time:

| Source type (already in `hdx-core`) | What MS5 reads from it | Module |
|---|---|---|
| `manifest::Manifest` (six private fields + getters) | `format_version`, `name`, `created_at` (`OffsetDateTime`), `producer_version`, `crs`, `cadence` | `manifest.rs` |
| `gridded_discovery::discover(path) -> Discovery` | the single combined model both verbs consume | `gridded_discovery.rs` |
| `Discovery::basins() -> &[BasinId]` | the basin list (scalar half's, sorted) | `gridded_discovery.rs` |
| `Discovery::fields() -> Vec<&Field>` | unified catalog `scalar ⊕ gridded`, concatenated (no merge) | `gridded_discovery.rs` |
| `Discovery::grids() -> &[GridInfo]` | per-grid extent/resolution/width/height/crs | `grid.rs` |
| `Discovery::delineations() -> &[DelineationLabel]` | distinct delineation labels | `gridded_discovery.rs` |
| `Discovery::scalar().per_basin() -> &[BasinScalar]` | per-basin folder id + `time_extent()` (`Option<TimeExtent>`) | `discovery.rs` |
| `Field` (`name`/`quadrant`/`dtype`/`units`/`grid_label`) | the inert field facts | `field.rs` |
| `TimeExtent` (`start()`/`end()` → `Timestamp::as_offset_date_time()`, `source()`) | per-basin `[start,end]` + its provenance | `scalar_reader.rs` |
| `ConsolidatedMetadataSource` (`Consolidated{members}` / `R3Skip{reason}`) | MED-5 honest path reporting (per dynamic artifact) | `zarr_reader.rs` |

**Decoded facts of the MS2 valid fixture (`conformance/valid/minimal/`)** — these
make the golden snapshot byte-true:

| Fact | Value |
|---|---|
| manifest | `{format_version:"0.1", name:"hdx-conformance-valid-minimal", created_at:"2026-06-01T00:00:00Z", producer_version:"hdx-fixtures 0.1.0", crs:"EPSG:4326", cadence:"daily"}` |
| basins | `["0001","0002","0003"]` |
| scalar fields | `drainage_area` (ScalarStatic/f64), `streamflow` (ScalarDynamic/f64) |
| gridded fields | `elevation` (GriddedStatic, label `era5`), `era5_precipitation` (GriddedDynamic, label `era5`), `era5_precipitation_was_filled` (GriddedDynamic, label `era5`) |
| unified field order | `drainage_area, streamflow, elevation, era5_precipitation, era5_precipitation_was_filled` |
| grid `era5` | extent west=10.0 north=50.0 east=11.5 south=48.0; res x=0.25 y=−0.25; width=6 height=8; crs `EPSG:4326` |
| per-basin time extents | 0001 `[2000-01-01,2000-01-05]`, 0002 starts `2010-06-15`, 0003 starts `2005-03-01` (ragged §6.1) — all `source == Statistics` |
| delineations | `{grit, merit}` |
| Zarr MED-5 path | `Consolidated` with 6 members (live) |
| wrong-format-version fixture | identical but `format_version:"0.2"` → MUST error `UnknownFormatVersion` before discovery |

**The R4 mini-contract decision this milestone adopts (S1).** The `Description` JSON
shape is a downstream contract (MS7 CLI, MS9 PyO3). To keep the wire shape owned by
the `describe` boundary — and to avoid coupling the inert domain types (`Field`,
`GridInfo`, `Manifest`, …) to a serialization format that could later drift — MS5
defines **describe-local `#[derive(Serialize)]` DTOs** that *mirror* the discovered
facts (the same two-stage discipline the manifest parser uses with `ManifestDto`).
The domain types stay free of `serde::Serialize`; the DTO layer is the single place
the JSON shape is defined, versioned **implicitly by `format_version` only**. This is
recorded as the R4 (describe half) decision.

**The floor stress-test discipline (spec §10/§11), made executable (S1/S3).** Every
field of `Description` must come from **either** one of the six manifest fields
**or** a discovered fact. The plan asserts this by construction: the describe DTO has
**no** field that is neither manifest-sourced nor discovery-sourced. If, while
assembling, a needed fact were found to be *neither* manifest nor discoverable, the
correct response is to **flag a spec/floor bug and record an architecture amendment —
never add a manifest field** (§11). A discovery gap (e.g. a basin with no time
extent, an absent outlines rollup) is reported as a **fact** (`null` / empty list),
never a verdict.

---

## Ordering rationale

MS5 turns the proven discovery layer into the first user-facing verb. The steps are
strictly dependency-sequential and each leaves the tree green (`cargo build` +
`cargo test` + `cargo clippy --all-targets -- -D warnings`) and is one conventional
commit with the mandated bump+tag:

1. **S1 — `Description` types + the serializable DTO layer (R4 shape frozen, no IO).**
   Stand up the `Description` domain struct (manifest + basins + fields + grids +
   time extents + delineations) **and** its describe-local `#[derive(Serialize)]`
   DTOs, with the pure mapping `Discovery + Manifest → Description → DTO`. Zero IO,
   zero new readers. This freezes the R4 wire shape **before** the verb is wired, so
   S2 (the verb) and S3 (the schema/golden) build against a settled contract. It also
   makes the floor stress-test reviewable in one place: every DTO field is annotated
   with its single source (a manifest field or a named discovery accessor). Mirrors
   the repo's parse-don't-validate / types-first discipline (MS1/MS4-S1).

2. **S2 — `describe(path)` boundary verb (§0 hard cut FIRST, then assemble + emit).**
   Implement `describe(path) -> Result<Description, DescribeError>` (and a sibling
   that emits the JSON string). It (a) reads `manifest.json` and boundary-parses it
   via `Manifest::from_json`, which **hard-cuts `format_version` first** (reject
   unknown with `UnknownFormatVersion`) — **before touching any other file**; then
   (b) runs `discover(path)` and maps `Discovery + Manifest → Description` via the S1
   layer. Facts only, no verdict. This depends on S1's shape and the MS4 discovery
   layer. Locked with tests over the valid fixture **and** the entry-discipline test
   that `describe` over `invalid/wrong-format-version/` errors `UnknownFormatVersion`
   before any discovery.

3. **S3 — `describe.schema.json` + golden output + jsonschema/snapshot tests (R4
   pinned).** Pin `schemas/describe.schema.json` (`additionalProperties:false`,
   mirroring the S1 DTO), commit the golden `describe` JSON of the MS2 valid fixture
   under `conformance/`, and add (a) a `jsonschema` dev-dep test that the golden
   validates against the schema and (b) a snapshot test that `describe` of the valid
   fixture equals the golden byte-for-byte. The golden snapshot is where the
   companion-mask (`era5_precipitation_was_filled`) and `{source}_{variable}`
   (`era5_precipitation`) fields are asserted to appear as **ordinary catalog fields
   with no special handling**. Depends on S2 (the verb produces the output to pin).

This order (shape → verb → contract-lock) is the same discovery-stack discipline as
the rest of the build and keeps each commit independently reviewable and green.

---

## Scope guard (read before every step)

- **No step exceeds MS5 or does MS6's work.** `describe` emits **facts only**; it
  contains **no** §14 rule and **no** `conformant` field. M1/M2 hard-cut *behavior*
  is exercised in `describe`'s entry path (it must reject an unknown version), but
  the §14 *checklist* (M3–M6, L*, I*, H*, T*, G*, Geo1 as pass/fail verdicts) is
  **MS6**. No `ValidationReport` type is introduced.
- **No later-milestone work.** No CLI (`main.rs` untouched — MS7); no PyO3 (MS9); no
  exhaustive invalid-fixture family (MS8). The only fixtures touched are the existing
  MS2 valid + wrong-format-version trees plus the new committed golden output.
- **No regrid / clip / reduce**, ever. No new gridded-chunk or pixel read — MS5 adds
  **no reader**; it reuses `discover()` and the MS4 metadata-only readers unchanged.
- **Inert / agnostic discipline holds.** No new type or field carries transform,
  role, semantic type, or provenance. The `Description` (and its DTO) is composed
  **only** from the six manifest fields + discovered facts; the manifest stays
  exactly six fields. If assembling reveals a missing fact, flag a floor bug and
  record an architecture amendment — never add a manifest field.
- **Do not reshape the discovery layer.** `Discovery`, `ScalarDiscovery`,
  `GriddedDiscovery`, `Field`, `GridInfo`, `TimeExtent` are MS3/MS4 contracts; MS5
  reads through their existing accessors and adds nothing to them.

---

## S1 — `Description` types + the serializable DTO layer (R4 shape, no IO)

**id.** MS5-S1

**Intent.** Freeze the R4 `describe` wire shape **before** the verb exists, so S2/S3
build against a settled contract — the same types-first discipline as MS1/MS4-S1.
Stand up the `Description` domain struct that mirrors architecture §3.5 (manifest,
basins, fields-with-quadrant, grids, per-basin ragged time extents, delineations) and
a describe-local `#[derive(Serialize)]` DTO layer that defines the JSON shape in one
place, leaving the inert domain types free of `serde::Serialize`. Add the pure
mapping `Discovery + Manifest → Description → DescriptionDto`. Zero IO, zero readers —
independently committable and green on unit tests over the mapping.

**Changes.**
- `crates/core/src/describe.rs` (new module; `pub mod describe;` added to `lib.rs`
  with a `//!`-referenced entry in the module map).
- In `describe.rs`:
  - `pub struct Description { manifest, basins, fields, grids, time_extents,
    delineations }` — fields private, exposed via getters; `#[derive(Debug, Clone,
    PartialEq)]`. Each member borrows/clones from the discovery layer; **no** derived
    or interpretive field. A per-basin time-extent entry pairs `BasinId` with
    `Option<TimeExtent>` (the §6.1 ragged fact; `None` = a recorded gap).
  - Private `#[derive(Serialize)]` DTOs (`DescriptionDto`, `ManifestDto`*, `FieldDto`,
    `GridDto`, `TimeExtentDto`, …) with `#[serde(deny_unknown_fields)]` not needed on
    output but field set is explicit. Quadrant serialized as the explicit 2×2
    (`{temporal, shape}` or a stable quadrant string — chosen and documented in S1);
    `Dtype` via its existing `as_str()`; `Units` as `string | null`; `grid_label` as
    `string | null`; `created_at` serialized as the RFC 3339 string (via `time`
    `Rfc3339` formatting) so the wire value matches the manifest input exactly.
    (*the manifest DTO is describe-local; it does not touch `manifest.rs`'s parser
    `ManifestDto`.)
  - `Description::from_discovery(&Manifest, &Discovery) -> Description` — the pure
    assembler (no IO): reads through the documented accessors only.
  - `Description::to_dto(&self) -> DescriptionDto` and a `to_json_string` /
    `to_json_pretty` helper that serializes the DTO via `serde_json`.
  - A module `//!` doc: the R4 mini-contract statement, the floor stress-test
    discipline (every field maps to a manifest field **or** a named discovery
    accessor — annotated per field), the facts-only / no-verdict rule, and a
    glossary. Each DTO field's doc names its single source.
- `crates/core/README.md` — add `describe` to the Mermaid module map + a glossary row
  (`Description`, `describe verb`, `R4 mini-contract`).
- `Cargo.toml` (root) — `./scripts/bump-version.sh patch`; stage alongside code.

**Test plan.**
- `from_discovery` over a **hand-built** `Discovery` (constructed from existing public
  constructors / the discovery accessors using small in-memory data, or — if a
  `Discovery` cannot be built purely in-memory without IO — over the result of
  `discover(conformance("valid/minimal"))`): asserts basins, the unified field order
  `scalar ⊕ gridded`, grids, ragged time extents, delineations are all mapped 1:1.
- A DTO-shape test: serialize the assembled `Description` of the valid fixture to a
  `serde_json::Value` and assert the **exact top-level key set** = `{manifest, basins,
  fields, grids, time_extents, delineations}` and **no `conformant` key** (facts-only,
  no verdict).
- A floor stress-test assertion (documented + executable): the DTO carries no field
  that is neither manifest-sourced nor discovery-sourced — pinned by asserting the
  key set per nested object (manifest = exactly the six floor keys; field = exactly
  `name/quadrant/dtype/units/grid_label`).
- Companion-mask / `{source}_{variable}` ordinariness: assert the serialized `fields`
  array contains `era5_precipitation` and `era5_precipitation_was_filled` as plain
  entries with the same key set as every other field (no extra `mask`/`belongs_to`).

**Acceptance.**
- `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings` all pass.
- `Description` and its DTO layer compile; domain types (`Field`, `GridInfo`,
  `Manifest`, `TimeExtent`) gain **no** `serde::Serialize` derive (the DTO owns the
  shape) and **no** new inert-violating field.
- The DTO top-level key set is exactly `{manifest, basins, fields, grids,
  time_extents, delineations}`; the manifest sub-object is exactly the six floor
  fields; a field sub-object is exactly `{name, quadrant, dtype, units, grid_label}`.
  No `conformant` / verdict key anywhere (advances spec §10 facts-only).
- The mapping does **not** reshape `Discovery` (only its public accessors are used).
- Commit via `./scripts/bump-version.sh patch` + stage `Cargo.toml` + conventional
  commit + `git tag v<version>`.

**Spec refs.** §2 (quadrant per field; ordinary fields, companion-mask &
`{source}_{variable}` are ordinary), §6.1 (per-basin ragged extents), §7 (per-grid
info), §9 (delineation labels), §10 (`describe` = facts only, the floor's stress
test), §11 (six-field floor — nothing derivable declared); R4; architecture §3.5/§5.

**Commit message.** `feat(core): add Description type and serializable describe DTO layer (R4 shape)`

---

## S2 — `describe(path)` boundary verb: §0 hard cut FIRST, then assemble + emit

**id.** MS5-S2

**Intent.** Wire the first verb: `describe(path)` reads `manifest.json`,
**hard-cuts `format_version` FIRST** via `Manifest::from_json` (reject unknown with
`UnknownFormatVersion`) **before touching any other file** (spec §0 entry
discipline), then runs the MS4 `discover(path)` and maps the result through the S1
assembler into a `Description`. Facts only — **no conformance verdict**. This is the
step that exercises the manifest floor as a stress test (§10): the assembler succeeds
using only the six manifest fields + discovered facts. Independently committable on
top of S1; green on fixture-backed tests.

**Changes.**
- `crates/core/src/describe.rs` — add the boundary functions:
  - `pub fn describe(path: impl AsRef<Path>) -> Result<Description, DescribeError>`,
    `#[instrument]`, `tracing` milestones (`info` on success, `debug` on stages).
    Order is load-bearing and documented: (1) read `<path>/manifest.json` to a string
    (IO error → typed error); (2) `Manifest::from_json` (hard cut + six-field parse) —
    **return immediately on error, before any discovery**; (3) `discover(path)`;
    (4) `Description::from_discovery(&manifest, &discovery)`.
  - `pub fn describe_json(path) -> Result<String, DescribeError>` (or
    `describe(path).map(|d| d.to_json_string())`) — the stable JSON string the CLI
    (MS7) / PyO3 (MS9) will surface.
- `crates/core/src/error.rs` — add a `DescribeError` (or extend the surface) with
  named-field variants, each doc-commented with *when* it fires:
  `ManifestUnreadable { path, detail }` (the `manifest.json` file is absent/unreadable
  — distinct from a *malformed* manifest), `Manifest(#[from] CoreError)` /
  `Discovery(#[from] CoreError)` wrapping (so the hard-cut `UnknownFormatVersion` and
  discovery `CoreError`s surface unchanged). **No `unwrap`/`expect`/panic** in library
  code. Decision recorded in S2: whether `DescribeError` is a thin wrapper enum over
  `CoreError` or `describe` returns `CoreError` directly with a new
  `ManifestUnreadable` variant — pick one in S2 and document it (both keep the §0 hard
  cut surfacing as `UnknownFormatVersion`).
- `lib.rs` module-map doc updated to describe the verb's entry order.

**Test plan.**
- **§0 entry-discipline test (FOLD-IN):** `describe(conformance("invalid/wrong-format-version"))`
  returns `Err(UnknownFormatVersion { found: "0.2" })`. To prove the cut happens
  **before** discovery, the test asserts the error is the version error (not a
  discovery error) — and a doc/comment records that `Manifest::from_json` runs and
  returns before `discover` is called (statically guaranteed by the function order).
- Valid-fixture happy path: `describe(conformance("valid/minimal"))` is `Ok`; assert
  the manifest round-trips (name/crs/cadence/created_at), basins
  `["0001","0002","0003"]`, the five-field unified catalog in order, the `era5` grid
  geometry (10.0/50.0/11.5/48.0, 6×8, EPSG:4326), three ragged extents all
  `source==Statistics`, delineations `{grit,merit}`.
- Facts-only / no-verdict: assert `describe_json` output, parsed back to a
  `serde_json::Value`, has **no** `conformant` key and no §14 check-outcome list.
- Manifest-unreadable path: `describe` over a temp dir with **no** `manifest.json`
  returns the typed `ManifestUnreadable` (not a panic, not a discovery error).
- Floor stress-test (executable): the valid-fixture `describe` succeeds with the
  manifest contributing exactly its six fields and every other datum sourced from
  `discover` — asserted by reconstructing the expected `Description` from
  `Manifest::from_json` + `discover` separately and `assert_eq!`-ing.

**Acceptance.**
- `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings` all pass.
- `describe` performs the **§0 hard version cut + manifest boundary-parse first**; a
  test confirms it errors `UnknownFormatVersion` on `format_version:"0.2"` **before**
  discovery (advances spec-check **M1/M2** hard-cut behavior *in describe's entry
  path* — not as a §14 verdict).
- `describe` emits **no** conformance verdict (facts only); discovery gaps are
  reported as facts (`null`/empty), never raised as a verdict.
- No `unwrap`/`expect`/panic in the new library code; all failures are typed errors
  with named fields, doc-commented with when they fire.
- The full §14 fact set is now *discoverable* through `describe` (basins, fields with
  quadrant, grids, time extents, units, delineations) — the discovery surface the
  validator (MS6) consumes.
- Commit via the bump+tag convention.

**Spec refs.** §0 (hard cut first; manifest read before anything else), §2 (quadrant
per field; ordinary fields), §5 (one-basin discovery surfaced), §6.1 (ragged time
extents), §7 (per-grid info), §9 (delineation labels), §10 (`describe` = the floor's
stress test, facts only), §11 (six-field floor); §14 M1/M2 (hard-cut behavior in the
entry path); architecture §5.

**Commit message.** `feat(core): implement describe verb with format_version hard cut before discovery`

---

## S3 — `describe.schema.json` + golden output + jsonschema/snapshot tests (R4 pinned)

**id.** MS5-S3

**Intent.** Lock the R4 mini-contract: pin `schemas/describe.schema.json` (mirroring
the S1 DTO, `additionalProperties:false`), commit the golden `describe` JSON of the
MS2 valid fixture, and assert both (a) the golden validates against the schema via the
existing `jsonschema` dev-dep and (b) `describe` of the valid fixture equals the
golden byte-for-byte (a snapshot test). The golden snapshot is the place the
companion-mask and `{source}_{variable}` fields are pinned as **ordinary** catalog
entries. Versioned implicitly by `format_version` only. Independently committable on
top of S2; green.

**Changes.**
- `schemas/describe.schema.json` (new) — JSON Schema for the `Description` output:
  the top-level object with required `{manifest, basins, fields, grids, time_extents,
  delineations}` and `additionalProperties:false`; nested schemas for the manifest
  (the six floor fields, `additionalProperties:false`), a field
  (`{name, quadrant, dtype, units, grid_label}`, `units`/`grid_label` nullable), a
  grid (extent/resolution/width/height/crs), a time-extent entry (`basin_id` +
  nullable `{start, end, source}`), and the delineation/basin arrays. Title +
  description cross-referencing spec §10/§11 and stating the shape is versioned by
  `format_version` only.
- `conformance/valid/minimal/describe.golden.json` (or
  `conformance/golden/valid-minimal.describe.json` — path chosen in S3 and documented
  in `conformance/README.md`) — the committed golden output, generated by the S2 verb
  (pretty-printed, deterministic key order).
- `conformance/README.md` — add a short "golden describe output" subsection: where the
  golden lives, that it is produced by `hdx-core`'s `describe` (not the Python
  generator), and the golden-update workflow note (MS8 extends this).
- `crates/core/src/describe.rs` (tests module) — the schema + snapshot tests.

**Test plan.**
- **R4 schema test (FOLD-IN, jsonschema dev-dep):** compile `schemas/describe.schema.json`
  with `jsonschema`; assert the **golden** `describe` output of the MS2 valid fixture
  **validates** against it.
- **Golden snapshot test:** `describe_json(conformance("valid/minimal"))` parsed to a
  `serde_json::Value` equals the committed golden parsed to a `Value` (compare as
  parsed JSON so formatting differences don't make it brittle, while still pinning the
  exact key/value content). A regeneration helper/comment documents how to refresh the
  golden when the shape legitimately changes (format_version bump only).
- **Companion-mask / `{source}_{variable}` ordinariness in the golden (FOLD-IN):**
  assert the golden's `fields` array entry for `era5_precipitation` and for
  `era5_precipitation_was_filled` each has **exactly** the ordinary field key set
  (`name/quadrant/dtype/units/grid_label`) — no `mask`, `companion`, `source`,
  `variable`, `belongs_to`, or any suffix/prefix-derived key. This is the snapshot
  pinning that these patterns get no special handling (spec §2).
- **Negative schema test:** a hand-mutated copy of the golden with an injected extra
  top-level key (or a `conformant` key) **fails** schema validation
  (`additionalProperties:false` works), proving the schema would catch a shape drift /
  an accidental verdict field.

**Acceptance.**
- `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings` all pass.
- `schemas/describe.schema.json` committed; the golden output committed; both asserted
  in Rust tests via the `jsonschema` dev-dep + a snapshot equality test (advances
  **R4** describe half).
- The golden snapshot pins the companion-mask (`era5_precipitation_was_filled`) and
  `{source}_{variable}` (`era5_precipitation`) fields as **ordinary** catalog fields
  with no special handling (spec §2).
- A mutated golden with an extra/`conformant` key is rejected by the schema (the shape
  + facts-only contract is enforced by the schema, not just by convention).
- The describe shape is versioned implicitly by `format_version` only (documented in
  the schema description + `conformance/README.md`).
- Commit via the bump+tag convention.

**Spec refs.** §2 (companion-mask & `{source}_{variable}` ordinary), §10 (facts only —
no verdict key in the schema), §11 (six-field manifest in the schema); R4; architecture
§5, §7 R4.

**Commit message.** `feat(core): pin describe.schema.json and golden describe output for the valid fixture`

---

## Coverage map — every MS5 deliverable / exit criterion / spec ref is assigned

| MS5 deliverable / exit criterion (milestones.md) | Step(s) |
|---|---|
| `describe(path) -> Result<Description, DescribeError>` reads `manifest.json` + hard-cuts `format_version` FIRST | S2 |
| Hard cut **before** touching any other file (spec §0 entry discipline) | S2 (function order; entry-discipline test) |
| Assemble `Description` from the MS3+MS4 discovery layer (basins / fields-with-quadrant / grids / ragged time extents / delineations) | S1 (assembler) + S2 (wired over `discover`) |
| Reports discovery **gaps as facts**, not verdicts | S1 (`Option<TimeExtent>` / empty lists) + S2 (no raised verdict) |
| `serde`-based JSON serialization; JSON shape = R4 mini-contract | S1 (DTO layer) + S3 (schema lock) |
| `schemas/describe.schema.json` | S3 |
| `jsonschema`-dev-dep test: golden validates against `describe.schema.json` | S3 |
| Golden `describe` output committed + snapshot test | S3 |
| `describe` over `invalid/wrong-format-version/` errors `UnknownFormatVersion` before any discovery | S2 |
| Companion-mask & `{source}_{variable}` fields appear as **ordinary** fields (asserted in golden snapshot) | S1 (DTO ordinariness) + S3 (golden assertion) |
| `describe` emits **no** conformance verdict (facts only) | S1 (no verdict key) + S2 (facts-only) + S3 (schema rejects a verdict key) |
| Floor stress-test: only six manifest fields + discovered facts; missing fact ⇒ flag floor bug, never add a manifest field | S1 (per-field source annotation + key-set asserts) + S2 (assembled from manifest ⊕ discover) |
| Full §14 fact set now discoverable; M1/M2 hard-cut behavior enforced in `describe`'s entry path | S2 |
| Output-schema stability versioned by `format_version` only | S3 |
| Every step: build + test + clippy `--all-targets -D warnings` + bump+tag | S1–S3 |

**Note on the serde-shape decision.** S1 deliberately keeps `serde::Serialize` off the
inert domain types and defines the JSON shape in a describe-local DTO layer (mirroring
the manifest parser's `ManifestDto` two-stage discipline). This makes the R4 contract a
single, reviewable surface that the schema (S3) mirrors 1:1, and prevents the wire
shape from silently coupling to — and drifting with — internal type changes. If a
future agent finds the DTO needs a field that is neither a manifest field nor a
discovery accessor result, that is a **floor/spec bug to flag (architecture
amendment)**, never a manifest addition (spec §11).
