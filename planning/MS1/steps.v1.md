# MS1 — Core types + manifest parse + manifest JSON Schema — STEP PLAN

> **Milestone:** MS1 (the first milestone of HDX v0.1; no dependencies).
> **Source contract:** `spec/HDX_SPEC.md` (canonical, settled).
> **Planned against:** `architecture.md` §3 (the type model) and `planning/milestones.md`
> (MS1 goal, deliverables, exit criteria, spec refs, risks).
> **Folded critique:** `planning/milestones-critique.md` MED-3 (prove *both* "too-many"
> and "too-few" manifest-field rejection) and the cross-cutting confirmations
> (inert/agnostic discipline, hard cut first, six-field floor, no derivable field).
>
> **Why this milestone is MS1.** The repo is at its initial state: `crates/core/src`
> contains only a one-line `lib.rs`, `schemas/` and `conformance/` hold only
> `.gitkeep`, `src/main.rs` is the scaffold `hello`. Nothing has been built. MS1 is
> the only milestone with no dependencies, so it is what is planned here.

---

## Scope guard

Every step below stays strictly inside MS1 (architecture §3 / milestones.md MS1):

- **No external IO whatsoever.** MS1 is pure types + JSON string parsing. No
  filesystem walk, no parquet/Zarr/COG/geoparquet reads — those are MS3/MS4. No
  step opens a dataset directory. (The `jsonschema` test reads a committed schema
  file and an in-test JSON literal; that is a unit-test asset read, not dataset IO.)
- **No verb logic.** No `describe` (MS5), no `validate` (MS6), no §14 rule engine.
  MS1 builds the *types the verbs will later stand on*, plus the manifest boundary
  parse (spec-checks M1–M4 foundations only) — it enforces no cross-file / cross-basin
  rule (M5, M6, L*, I*, H*, T*, G*, Geo1 are all later).
- **No CLI changes** (MS7). `src/main.rs` is untouched.
- **No later-milestone work.** No regrid/clip/reduce/reduction/hydrology anywhere
  (excluded forever, spec §10). No reader crates (`parquet`/`arrow`/`zarrs`/`tiff`/
  `geoarrow`) are added — those land in MS3/MS4.
- **Inert/agnostic discipline (hard rule, spec §1/§13).** No type or field carries
  transform / normalization / role / semantic-type / reduction / provenance /
  computation-source. The `Manifest` is *exactly* the six floor fields (§11); no
  derivable field (no content hash, no data-version, no field catalog, no basin
  list) is ever added to it. `Field` carries only `name`, `quadrant`, `dtype`,
  `units`, `grid_label` (architecture §3.3) — no role/transform/semantic field.
  `Dtype` is opaque to semantics (no continuous/categorical). `Units` is an opaque
  optional string (no parsing, no vocabulary). `FieldName`/`GridLabel`/
  `DelineationLabel` are opaque producer strings (HDX parses none).
- **`format_version` is a HARD cut.** `FormatVersion` has exactly one arm (`V0_1`);
  the parse succeeds only on `"0.1"` and errors (`UnknownFormatVersion`) on anything
  else. No multi-version path is representable.

No step performs a later milestone's work, and none violates the inert/agnostic
discipline.

---

## Ordering rationale

The steps follow strict type-dependency, bottom-up, so each commit compiles and the
repo stays green:

1. **S1 — newtypes + error skeleton + module scaffold first.** Everything else
   references the opaque newtypes and the `thiserror` error enum. With zero
   behavior to test, this step is anchored by `Debug/Clone/PartialEq` derives,
   constructor round-trips, and the build/clippy gate. It also wires the module tree
   in `lib.rs` so later steps slot in without churn.
2. **S2 — `FormatVersion` hard cut.** Needs the error enum (S1) for
   `UnknownFormatVersion`. Small, sharply testable (the §0/M2 hard cut), and a
   prerequisite of the `Manifest` (S4), so it lands before the manifest.
3. **S3 — the field 2×2 (`Temporal`/`Shape`/`Quadrant`/`Dtype`/`Units`/`Field`).**
   Needs `FieldName`/`GridLabel` (S1) and the error enum (S1, for `UnknownDtype`).
   Independent of `FormatVersion`, but placed after S2 to keep the manifest (which
   needs S2) as the next coherent unit. Encodes the `grid_label.is_some() ⇔
   Shape::Gridded` invariant and the closed `Dtype` fallible parse.
4. **S4 — `Manifest` boundary parse.** Needs `FormatVersion` (S2) and the
   `DatasetName`/`ProducerVersion`/`Crs`/`Cadence` newtypes (S1). Adds the first
   external crates (`serde`/`serde_json` + a strict RFC 3339 time crate). Implements
   `deny_unknown_fields` (rejects too-many, M3) *and* required-field presence
   (rejects too-few, M3 — folds MED-3), `format_version` read+cut first (M1/M2),
   `created_at` RFC 3339 + non-empty `crs`/`cadence` (M4).
5. **S5 — `schemas/manifest.schema.json` + `jsonschema` dev-dep test.** Needs the
   `Manifest` (S4) so the schema and the Rust parser can be asserted to agree. Pins
   R4 (manifest half) and proves *both* directions of M3 against *both* the schema
   and the parser (folds MED-3).
6. **S6 — `crates/core/README.md` (Mermaid module map + glossary).** Pure docs, no
   behavior; placed last so the module map and glossary reflect the final MS1 shape
   (all modules from S1–S5 exist). Independently committable (docs-only) and leaves
   the repo green.

Each step is one conventional commit, ends with `./scripts/bump-version.sh patch` +
stage `Cargo.toml` + commit + `git tag v<version>` (CLAUDE.md / architecture §2), and
after it `cargo build` + `cargo test` + `cargo clippy --all-targets -- -D warnings`
all pass.

---

## Steps

### MS1-S1 — Newtypes, error-enum skeleton, and module scaffold

**Intent.** Stand up the opaque domain newtypes and the `thiserror` error enum that
every later type references, and wire the `hdx-core` module tree so subsequent steps
slot in without restructuring. This is the floor of the floor: it leaves the crate
green, compiles, and is reviewable as "the type vocabulary + error surface, with no
behavior yet." Independently committable because it adds compiling, tested types and
changes nothing downstream.

**Changes.**
- `crates/core/src/lib.rs` — replace the one-line stub with the module declarations
  (`pub mod newtypes;`, `pub mod error;`, plus `pub mod format_version;`,
  `pub mod field;`, `pub mod manifest;` declared as they are added in later steps;
  S1 declares `newtypes` + `error` and a module-level `//!` purpose comment).
- `crates/core/src/newtypes.rs` — new. The opaque newtypes from architecture §3.1:
  `BasinId`, `FieldName`, `GridLabel`, `DelineationLabel`, `Crs`, `Cadence`,
  `DatasetName`, `ProducerVersion`. Each wraps a `String`, derives
  `Debug, Clone, PartialEq` (and `Eq, Hash` where used as map keys, e.g. `BasinId`,
  `GridLabel`), has a constructor (`new(impl Into<String>)`) and an `as_str()`
  accessor; field is private. Module `//!` purpose line. No `role`/`transform`/
  `semantic` field on any of them (inert/agnostic).
- `crates/core/src/error.rs` — new. `#[derive(Debug, thiserror::Error)]` enum
  `CoreError` with named-field variants, each doc-commented with *when* it fires:
  `UnknownFormatVersion { found: String }`, `ExtraManifestField { field: String }`,
  `MissingManifestField { field: String }`, `InvalidTimestamp { value: String }`,
  `EmptyCrs`, `EmptyCadence`, `UnknownDtype { found: String }`, plus
  not-yet-fired stubs for later milestones: `BasinIdFolderMismatch`, `RaggedSchema`,
  `GridLabelMismatchAcrossBasins`, `MissingRootRollup`, `NonMonotonicTime` (each
  documented as "fires in MSn" so reviewers know they are intentional skeletons).
  `#[allow(dead_code)]` (or a `// reserved for MSn` doc) on the stub variants so
  clippy stays green without an unused-variant warning, if needed.

**Test plan.**
- Unit tests in `newtypes.rs`: each newtype round-trips (`X::new("v").as_str() == "v"`)
  and `PartialEq` works; `BasinId`/`GridLabel` usable as `HashSet`/`HashMap` keys.
- A compile-level assertion (test that constructs each error variant) so the enum
  shape is exercised and the stub variants are referenced (keeps clippy quiet,
  documents intent).
- `cargo test -p hdx-core`, `cargo clippy --all-targets -- -D warnings`.

**Acceptance.**
- `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings` all pass.
- All eight newtypes exist, are opaque (private field), derive
  `Debug, Clone, PartialEq`, and round-trip in a test.
- `CoreError` exists with the named-field, doc-commented variants listed; no variant
  is a tuple variant; no `unwrap`/`expect`/`panic` anywhere in the crate.
- `lib.rs` declares the module tree with a `//!` purpose comment.
- Advances spec-check foundations only by establishing the typed vocabulary; enforces
  none yet. (Inert/agnostic: a reviewer can confirm no type carries transform/role/
  semantic/provenance.)
- Patch-bump + stage `Cargo.toml` + conventional commit + `git tag v<version>`.

**Spec refs.** §1 (inert/agnostic — newtypes carry no interpretation), §2 (field is
the unit; `FieldName`/`GridLabel` opaque), §3 (`BasinId` opaque/unique-later), §9
(`DelineationLabel` neutral), architecture §3.1 (newtypes), §3.6 (thiserror,
named-field, no `unwrap`/`expect`).

**Commit message.** `feat(core): add domain newtypes and error-enum skeleton`

---

### MS1-S2 — `FormatVersion` hard version cut

**Intent.** Encode the spec's hardest invariant — `format_version` is a hard cut
(§0/§14 M2) — as a single-arm enum whose parse succeeds only on `"0.1"` and errors
otherwise. Independently committable: it adds the `FormatVersion` type and its
fallible parse plus tests, and nothing else depends on it being wired into the
manifest yet (that is S4).

**Changes.**
- `crates/core/src/format_version.rs` — new. `pub enum FormatVersion { V0_1 }` with
  `Debug, Clone, Copy, PartialEq, Eq` derives. A fallible parse: implement
  `FromStr` (and/or `TryFrom<&str>`) returning
  `Result<FormatVersion, CoreError>` — `Ok(FormatVersion::V0_1)` for `"0.1"`,
  `Err(CoreError::UnknownFormatVersion { found })` for anything else. An
  `as_str()`/`Display` yielding `"0.1"`. `#[instrument]` is unnecessary on a pure
  parse; a `debug!`/`warn!` on the reject path is allowed (tracing only, never
  `println!`). Doc comment states it is the only contract-version axis and the hard
  cut (no multi-version reader is representable).
- `crates/core/src/lib.rs` — add `pub mod format_version;`.

**Test plan.**
- `FormatVersion::from_str("0.1")` (or `TryFrom`) returns `Ok(V0_1)`.
- `"0.2"`, `"1.0"`, `""`, `"0.1.0"`, `"0.10"` each return
  `Err(UnknownFormatVersion { found })` with `found` echoing the input — the §0/M2
  hard cut. (Note `"0.10"` ≠ `"0.1"`: exact-string match, no numeric coercion.)
- `as_str()`/`Display` round-trips to `"0.1"`.
- `cargo test -p hdx-core`, `cargo clippy --all-targets -- -D warnings`.

**Acceptance.**
- `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings` all pass.
- Parsing `"0.1"` succeeds; every other string errors with `UnknownFormatVersion`
  (no panic, no default) — **spec-check M2 hard cut** is enforced at the type level.
- Foundation for **spec-check M1** ("`format_version` read first") — the type exists;
  the "read first" ordering is wired into the manifest parse in S4.
- Patch-bump + stage `Cargo.toml` + conventional commit + `git tag v<version>`.

**Spec refs.** §0 (read first; hard cut; no multi-version readers), §11 (`"0.1"`
here), §14 M1 (foundation), §14 M2 (hard cut), architecture §3.2.

**Commit message.** `feat(core): add FormatVersion hard version cut`

---

### MS1-S3 — Field 2×2: `Temporal`, `Shape`, `Quadrant`, `Dtype`, `Units`, `Field`

**Intent.** Encode the field model — enums over booleans for the two axes, the four
quadrants, a closed `Dtype` with a fallible boundary parse, opaque `Units`, and the
`Field` struct with the `grid_label.is_some() ⇔ Shape::Gridded` invariant enforced in
its constructor. Independently committable: adds the field types + their tests; no
later type depends on it yet (the manifest does not embed fields — the field catalog
is *discovered*, never declared, §11).

**Changes.**
- `crates/core/src/field.rs` — new:
  - `pub enum Temporal { Static, Dynamic }` and `pub enum Shape { Scalar, Gridded }`
    (enums over booleans, §2), both `Debug, Clone, Copy, PartialEq, Eq`.
  - `pub enum Quadrant { ScalarStatic, ScalarDynamic, GriddedStatic, GriddedDynamic }`
    with helpers `Quadrant::from_axes(Temporal, Shape) -> Quadrant`,
    `.temporal()`, `.shape()` — derive the per-field classification, never a
    dataset-level mode (§2; architecture §3.3).
  - `pub enum Dtype { F32, F64, I32, I64, Bool, Timestamp }` — a **closed** enum,
    opaque to semantics (no continuous/categorical). Doc comment states the
    no-panic guarantee for unknowns and the *reject-don't-carry* policy (no
    `Other(String)`). A fallible boundary parse
    `parse_dtype(&str) -> Result<Dtype, CoreError>` mapping documented physical-type
    strings (e.g. `"float32"|"f32"` → `F32`, …) and returning
    `Err(CoreError::UnknownDtype { found })` on anything unmapped — no panic, no
    silent default.
  - `pub struct Units(Option<String>)` — opaque optional string, `Debug, Clone,
    PartialEq`; constructor `Units::new(Option<String>)` / `Units::none()`; no
    parsing, no vocabulary (inert/agnostic).
  - `pub struct Field { name: FieldName, quadrant: Quadrant, dtype: Dtype, units:
    Units, grid_label: Option<GridLabel> }` — private fields, `Debug, Clone,
    PartialEq`. Constructor `Field::new(...) -> Result<Field, CoreError>` (or a
    `FieldError` variant) enforcing the invariant
    `grid_label.is_some() == matches!(quadrant.shape(), Shape::Gridded)`; mismatch is
    a typed error (add `MismatchedGridLabel { quadrant, has_label }` to `error.rs`,
    doc-commented). Accessors for each field. **No** role/transform/semantic field.
- `crates/core/src/error.rs` — add the `MismatchedGridLabel` variant (named fields,
  doc-commented with when it fires).
- `crates/core/src/lib.rs` — add `pub mod field;`.

**Test plan.**
- `Quadrant::from_axes` covers all four combinations; `.temporal()`/`.shape()`
  round-trip.
- `parse_dtype` maps each documented physical-type string to the right `Dtype`;
  an unknown string (e.g. `"complex128"`, `""`) returns `UnknownDtype { found }` and
  **never panics** (assert via `Result`, not `#[should_panic]`).
- `Field::new` with `Shape::Gridded` quadrant + `Some(grid_label)` succeeds;
  `Gridded` + `None` errors; `Scalar` + `Some(label)` errors; `Scalar` + `None`
  succeeds — the `grid_label ⇔ Gridded` invariant.
- `Units::none()` and `Units::new(Some(..))` round-trip; no parsing occurs.
- `cargo test -p hdx-core`, `cargo clippy --all-targets -- -D warnings`.

**Acceptance.**
- `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings` all pass.
- `Temporal`/`Shape`/`Quadrant` are enums (no bool); `Dtype` is a **closed** enum
  with a **no-panic fallible** parse (`UnknownDtype` on unknown) — satisfies
  milestones.md MS1 Dtype requirement + CLAUDE.md "no `unwrap`/`expect` in library
  code."
- `Field` enforces `grid_label.is_some() ⇔ Shape::Gridded` in its constructor
  (invalid combinations unrepresentable) — typed-error, not panic.
- Field-model scaffolding for **spec-check H1** (field schema = names/dtypes/
  quadrants) is *typed*; enforcement of cross-basin identity is MS6.
- Inert/agnostic confirmed: `Field` carries only name/quadrant/dtype/units/
  grid_label; `Dtype` is semantics-opaque; `Units` is unparsed.
- Patch-bump + stage `Cargo.toml` + conventional commit + `git tag v<version>`.

**Spec refs.** §2 (field 2×2; quadrant per field, never a dataset mode; companion
masks / `{source}_{variable}` are *ordinary* fields — no special casing here), §1
(no semantic types; `Dtype`/`Units` opaque), §14 H1 (typed foundation), architecture
§3.3.

**Commit message.** `feat(core): add field 2x2 quadrant model and closed Dtype parse`

---

### MS1-S4 — `Manifest` six-field boundary parse (reject extra AND missing)

**Intent.** Implement the manifest boundary parse — the one piece of MS1 that turns
raw JSON into a domain type. Exactly the six floor fields (§11); `format_version`
read + hard-cut **first** (M1/M2); `deny_unknown_fields` rejects a 7th field (M3
too-many); explicit required-field presence rejects a 5-field manifest (M3 too-few —
**folds MED-3**); `created_at` parsed as strict RFC 3339 (M4); `crs`/`cadence`
non-empty (M4). Independently committable: adds the `Manifest` type + its parser +
tests; introduces the first external crates.

**Changes.**
- `crates/core/Cargo.toml` — add `serde = { version = "1", features = ["derive"] }`,
  `serde_json = "1"`, and a strict RFC 3339 time crate (`time` with `parsing`/
  `serde` + `format_description`/`well-known::Rfc3339`, **or** `chrono` with
  `DateTime::parse_from_rfc3339`) — pin one and record the choice in the doc comment
  / README (milestones.md MS1 risk: "pin the crate in MS1 to avoid churn"). **No
  reader crates** (`parquet`/`arrow`/`zarrs`/`tiff`) — those are MS3/MS4.
- `crates/core/src/manifest.rs` — new:
  - `pub struct Manifest { format_version: FormatVersion, name: DatasetName,
    created_at: <Rfc3339 datetime>, producer_version: ProducerVersion, crs: Crs,
    cadence: Cadence }` — exactly six fields, private, `Debug, Clone, PartialEq`,
    accessors. **No** seventh/derivable field (no content hash, no data-version, no
    field catalog) — inert/agnostic + §11 floor.
  - A `#[serde(deny_unknown_fields)]` raw DTO (all-`String`/raw) used for parsing,
    then a `Manifest::from_json(&str) -> Result<Manifest, CoreError>` boundary parse
    that: (1) reads `format_version` and hard-cuts via S2 **before** validating the
    rest (M1/M2 ordering); (2) maps a serde missing-field error to
    `MissingManifestField { field }` and an unknown-field error to
    `ExtraManifestField { field }` (M3 both directions); (3) parses `created_at` as
    strict RFC 3339 → `InvalidTimestamp` on failure (M4); (4) rejects empty
    `crs`/`cadence` → `EmptyCrs`/`EmptyCadence` (M4). `#[instrument(skip(json))]` on
    the public parse; `debug!`/`warn!` on reject paths (tracing only).
- `crates/core/src/lib.rs` — add `pub mod manifest;`.

**Test plan.**
- The §11 example manifest (the exact six-field JSON block in the spec) parses to a
  `Manifest` and its accessors return the expected values (round-trip).
- A 7-field manifest (six + one extra key, e.g. `content_hash`) → `ExtraManifestField`
  (M3 too-many).
- A 5-field manifest (one floor field omitted, e.g. no `cadence`) →
  `MissingManifestField { field: "cadence" }` (M3 too-few — **MED-3**).
- `format_version: "0.2"` → `UnknownFormatVersion` and this error fires **before**
  any other field is validated (assert by giving an otherwise-also-broken manifest,
  e.g. `"0.2"` + empty `crs`, and checking the *version* error is returned) (M1/M2
  read-first ordering).
- A malformed `created_at` (e.g. `"2026-06-01"` date-only, or `"not-a-date"`) →
  `InvalidTimestamp`; the `Z`-form RFC 3339 example parses (M4).
- Empty `crs` → `EmptyCrs`; empty `cadence` → `EmptyCadence` (M4).
- `cargo test -p hdx-core`, `cargo clippy --all-targets -- -D warnings`.

**Acceptance.**
- `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings` all pass.
- **spec-check M1** (manifest is valid JSON; `format_version` read first), **M2**
  (hard cut applied first), **M3** (exactly six — rejects *both* a 7-field and a
  5-field manifest; MED-3), **M4** (`created_at` RFC 3339; `crs`/`cadence` non-empty)
  are enforced *at the boundary parse* (no cross-file checks — M5/M6 are MS6).
- No `unwrap`/`expect`/`panic`; all failures are typed `CoreError` variants.
- `Manifest` has exactly the six fields; the RFC 3339 crate is pinned and recorded.
- Patch-bump + stage `Cargo.toml` + conventional commit + `git tag v<version>`.

**Spec refs.** §0 (read `format_version` first; hard cut), §11 (exactly six floor
fields; nothing derivable), §14 M1, M2, M3 (`deny_unknown_fields` + required-field
presence, both directions — MED-3), M4 (RFC 3339; non-empty `crs`/`cadence`),
architecture §3.4, §3.6.

**Commit message.** `feat(core): add six-field Manifest boundary parser`

---

### MS1-S5 — `manifest.schema.json` + `jsonschema` dev-dep test (R4 manifest half)

**Intent.** Pin the manifest JSON Schema (R4, manifest half) and prove — in a Rust
test via a `jsonschema` dev-dependency — that the committed schema and the S4 parser
agree in *both* M3 directions: the §11 example validates against both, a 7-field
manifest is rejected by both, and a 5-field manifest is rejected by both (folds
MED-3). Independently committable: adds a schema asset + a dev-dep + a test; no
production behavior change.

**Changes.**
- `schemas/manifest.schema.json` — new. JSON Schema (draft 2020-12 or draft-07,
  pinned) for `manifest.json`: `"type": "object"`, `"additionalProperties": false`,
  `"required"` listing **all six** floor fields, and a `properties` block typing each
  (`format_version` as `const "0.1"`, `created_at` as `string` + `format: date-time`,
  `crs`/`cadence`/`name`/`producer_version` as non-empty `string` via `minLength: 1`).
  No seventh/derivable property is permitted (§11 floor mirrored in the schema).
- `crates/core/Cargo.toml` — add `jsonschema` as a **`[dev-dependencies]`** entry
  (test-only; never in shipped `hdx-core`). Pin the version.
- `crates/core/tests/manifest_schema.rs` (or a `#[cfg(test)]` module that reads the
  schema file by path relative to `CARGO_MANIFEST_DIR`) — a test that:
  - loads `schemas/manifest.schema.json`, compiles it with `jsonschema`;
  - asserts the §11 example manifest **validates** against the schema;
  - asserts a **7-field** manifest **fails** schema validation (M3 too-many) **and**
    the S4 parser (`ExtraManifestField`);
  - asserts a **5-field** manifest (one field omitted) **fails** schema validation
    (M3 too-few — MED-3: schema `required` must list all six) **and** the S4 parser
    (`MissingManifestField`).

**Test plan.**
- The four assertions above (valid; 7-field fail×2; 5-field fail×2) — schema and
  parser cross-checked, *both* M3 directions.
- `cargo test -p hdx-core` (runs the schema test), `cargo clippy --all-targets --
  -D warnings`.

**Acceptance.**
- `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings` all pass.
- `schemas/manifest.schema.json` is committed, has `additionalProperties: false`,
  and lists all six fields in `required` (so it rejects *both* too-many and too-few
  — MED-3).
- The `jsonschema` dev-dep test asserts schema↔parser agreement in both M3
  directions (R4 manifest half pinned).
- **spec-checks M1–M4** now have a committed, test-asserted schema mirror in addition
  to the S4 parser. No production code path depends on `jsonschema` (dev-only).
- Patch-bump + stage `Cargo.toml` + conventional commit + `git tag v<version>`.

**Spec refs.** §11 (six-field floor mirrored in the schema; nothing derivable),
§14 M1–M4 (schema mirror), milestones.md MS1 (R4 manifest half, `jsonschema` dev-dep,
MED-3 both-directions), architecture §7 R4.

**Commit message.** `test(core): pin manifest JSON Schema and assert parser agreement`

---

### MS1-S6 — `crates/core/README.md` (Mermaid module map + glossary)

**Intent.** Document `hdx-core` as the agent entry-point for the crate: a one-paragraph
purpose, a Mermaid module map of the MS1 modules, and a glossary of the domain terms
an LLM agent would not infer from the code. Placed last so the map and glossary reflect
the final MS1 shape (all of S1–S5 exist). Independently committable: docs-only, no
behavior change, repo stays green.

**Changes.**
- `crates/core/README.md` — new. Sections per CLAUDE.md "crate-level README":
  - **Purpose** — one paragraph: `hdx-core` holds all contract logic for HDX v0.1
    (`validate` + `describe` land later); MS1 establishes the parse-don't-validate
    type model + the six-field manifest boundary parser.
  - **Architecture** — a **Mermaid** module map (`newtypes`, `error`,
    `format_version`, `field`, `manifest`) showing dependencies (manifest → format_version
    + newtypes; field → newtypes + error; all → error). Mermaid, never ASCII art.
  - **Glossary** — table of domain terms: *field*, *quadrant* (the 2×2), *Temporal/
    Shape*, *Dtype* (closed, semantics-opaque), *Units* (opaque optional), *basin_id*,
    *grid label* (shared label ⇒ alignment, enforced MS6), *delineation* (neutral
    label), *cadence*, *manifest floor* (exactly six fields), *format_version hard cut*.
  - A short **inert/agnostic** note: HDX records *shape*, never *what was done* — no
    transform/role/semantic/provenance type or field exists.

**Test plan.**
- No Rust test. Verify the Mermaid block is well-formed (fenced ` ```mermaid `) and
  the module names match the actual `src/*.rs` files.
- `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings` still pass
  (docs-only change cannot break them, but the gate is re-run).

**Acceptance.**
- `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings` all pass.
- `crates/core/README.md` exists with Purpose + a **Mermaid** module map + a glossary
  covering the listed domain terms (CLAUDE.md docs convention for the complex crate).
- The README states the inert/agnostic discipline and the six-field floor explicitly.
- No spec-check advanced (docs); this completes the MS1 deliverable set.
- Patch-bump + stage `Cargo.toml` + conventional commit + `git tag v<version>`.

**Spec refs.** §1 (inert/agnostic note), §2 (field/quadrant glossary), §11 (manifest
floor glossary), milestones.md MS1 (README started: Mermaid module map + glossary),
CLAUDE.md (crate-level README, Mermaid not ASCII).

**Commit message.** `docs(core): add crate README with module map and glossary`

---

## Coverage check — every MS1 deliverable & exit criterion is assigned

| MS1 deliverable / exit criterion | Step |
|---|---|
| Newtypes `BasinId`/`FieldName`/`GridLabel`/`DelineationLabel`/`Crs`/`Cadence`/`DatasetName`/`ProducerVersion` | S1 |
| `thiserror` error enum skeleton (named-field, doc-commented, with later-MS stubs) | S1 (+ `MismatchedGridLabel` in S3) |
| `FormatVersion` enum, `V0_1` only, fallible parse / hard cut (M1/M2) | S2 |
| `Temporal`/`Shape`/`Quadrant` enums (enums over booleans) | S3 |
| Closed `Dtype` + fallible `parse_dtype` (no-panic, `UnknownDtype`) | S3 |
| `Units` (opaque optional) | S3 |
| `Field` with `grid_label ⇔ Gridded` invariant in constructor | S3 |
| `Manifest` six-field parser, `deny_unknown_fields`, RFC 3339, non-empty crs/cadence (M1–M4) | S4 |
| Reject *both* too-many (M3) and too-few (M3, MED-3) manifest | S4 (parser) + S5 (schema) |
| `format_version` read + hard-cut **first** (M1/M2 ordering) | S4 |
| `schemas/manifest.schema.json` committed | S5 |
| `jsonschema` dev-dep + test asserting schema↔parser agreement (R4) | S5 |
| `crates/core/README.md` (Mermaid module map + glossary) | S6 |
| `cargo build`+`test`+`clippy -D warnings` green after each step | S1–S6 (every step) |
| Bump+tag commit discipline | S1–S6 (every step) |
| Inert/agnostic discipline; six-field floor; no derivable field | S1–S6 (scope guard, enforced per step) |

**Exit-criteria spec MUST-checks (MS1 advances/establishes):** M1, M2 (enforced at the
type + parse boundary in S2/S4/S5); M3 both directions (S4 parser + S5 schema, MED-3);
M4 (S4 + S5). H1 field-model scaffolding is *typed* (S3) but *enforced* in MS6. All
other §14 checks (M5, M6, L*, I*, H2, T*, G*, Geo1) are out of MS1 scope (no IO) and
are explicitly deferred to MS3–MS6.
