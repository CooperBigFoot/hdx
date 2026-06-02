# MS5 STEP-plan critique (adversarial review)

**Verdict: APPROVED (with low/medium advisory issues; zero high/critical).**

Milestone: MS5 — `describe`: assemble + emit the full self-description (+ describe JSON Schema).
Reviewed: `planning/MS5/steps.md` against `spec/HDX_SPEC.md`, `architecture.md`,
and the MS5 section of `planning/milestones.md`, cross-checked against the committed
`crates/core` source and the MS2 fixture tree.

---

## Ground-truth verification (the plan's claims vs the committed code)

Every API the plan's "Ground truth" table relies on was verified to exist with the
claimed shape:

- `Manifest::from_json` (manifest.rs:106) parses in the documented order — **stage 2a
  hard-cuts `format_version` FIRST** (manifest.rs:110-112) before any other field is
  interpreted. Confirms the S2 §0-first claim is buildable.
- `discover(path) -> Result<Discovery, CoreError>` (gridded_discovery.rs:589),
  `Discovery::basins()` (548, scalar half, sorted), `Discovery::fields()` (555,
  `scalar ⊕ gridded` concatenated, no merge), `Discovery::grids()` (564),
  `Discovery::delineations()` (569).
- `Discovery::scalar().per_basin() -> &[BasinScalar]` (discovery.rs:195) and
  `BasinScalar::time_extent() -> Option<TimeExtent>` (151) — the ragged §6.1 fact,
  `None` = recorded gap.
- `Field` getters `name/quadrant/dtype/units/grid_label` (field.rs:283-303);
  `Dtype::as_str()` (133); `Units::as_deref()` (208).
- `GridInfo` getters `grid_label/extent/resolution/width/height/crs` (grid.rs:235-260).
- `TimeExtent::start/end/source` (scalar_reader.rs:180-190); `TimeExtentSource`
  variants are `Statistics | BoundedColumnScan` (154-163) — the plan's "all
  `source == Statistics`" golden fact is the `Statistics` variant.
- `ConsolidatedMetadataSource::{Consolidated{members}, R3Skip{reason}}`
  (zarr_reader.rs:111) — available per dynamic artifact.

**Critical confirmation for the §0 fold-in:** the discovery layer does **not** read
`manifest.json` (layout.rs:541 comment: *"the mutation lives only in manifest.json,
which the walk never reads"*). Therefore the wrong-format-version fixture (which
differs only in `manifest.json`) would pass `discover()` cleanly — the only thing
that rejects it is the `describe` boundary's manifest parse. This makes the S2
entry-discipline test (`describe(wrong-format-version)` → `UnknownFormatVersion`
**before** discovery) genuinely load-bearing, not a tautology.

The MS2 golden facts the plan pins (field order
`drainage_area, streamflow, elevation, era5_precipitation, era5_precipitation_was_filled`,
shared `era5` grid label, basins `["0001","0002","0003"]`, delineations `{grit, merit}`,
grid extent `10.0/50.0/11.5/48.0`, `6×8`, `EPSG:4326`) are all confirmed by existing
`gridded_discovery.rs` / `scalar_reader.rs` tests. The golden snapshot is byte-true.

Baseline `cargo build -p hdx-core` is green.

---

## STEP-2 fold-in verification (each required fold checked, not cosmetic)

### Fold 1 — §0 hard cut FIRST (entry discipline) — GENUINELY FOLDED
- S2 documents a load-bearing order: (1) read `<path>/manifest.json`; (2)
  `Manifest::from_json` (hard cut + six-field parse) — **return immediately on error,
  before any discovery**; (3) `discover(path)`; (4) `from_discovery`.
- The §0 entry-discipline test is explicit and labeled `(FOLD-IN)`:
  `describe(conformance("invalid/wrong-format-version"))` →
  `Err(UnknownFormatVersion { found: "0.2" })`, asserting the **version** error (not a
  discovery error). Verified meaningful (discovery never reads the manifest).
- Facts-only / no-verdict asserted three ways: S1 (no `conformant` key in the DTO
  key-set test), S2 (parsed-Value has no `conformant` key / no §14 outcome list), S3
  (schema `additionalProperties:false` rejects an injected `conformant` key). Matches
  the requirement that describe emits FACTS ONLY.

### Fold 2 — Floor stress-test (spec §10/§11) — GENUINELY FOLDED
- S1 "Test plan" + "Acceptance" pin the floor by construction: per-nested-object
  key-set asserts (manifest = exactly the six floor keys; field = exactly
  `name/quadrant/dtype/units/grid_label`), and each DTO field's doc names its single
  source (a manifest field or a named discovery accessor).
- S2 adds the executable stress-test: reconstruct the expected `Description` from
  `Manifest::from_json` + `discover` separately and `assert_eq!` — proving every datum
  is sourced from the six manifest fields ⊕ discovery.
- The "missing fact ⇒ flag a floor bug + architecture amendment, NEVER add a manifest
  field" discipline is stated in the milestone-scope header, the Scope guard, the S1
  intent, the coverage map, and the closing note. Reported as facts (`null`/empty),
  never a verdict.

### Fold 3 — R4 (describe output schema stability) — GENUINELY FOLDED
- S3 pins `schemas/describe.schema.json` (`additionalProperties:false`, mirroring the
  S1 DTO) and validates the **golden** describe output of the MS2 valid fixture against
  it via the existing `jsonschema` dev-dep (labeled `(FOLD-IN)`).
- Versioned implicitly by `format_version` only (documented in the schema description +
  `conformance/README.md`).
- Companion-mask (`era5_precipitation_was_filled`) and `{source}_{variable}`
  (`era5_precipitation`) ordinariness pinned in the golden snapshot (S3) and in the DTO
  key-set test (S1) — asserted to have **exactly** the ordinary field key set, no
  `mask`/`companion`/`source`/`variable`/`belongs_to`. Matches the requirement that
  these appear as ORDINARY catalog fields with no special handling.
- A negative schema test (mutated golden with an extra/`conformant` key fails
  validation) proves the schema actually enforces the shape, not just convention.

All three folds are substantive, not cosmetic.

---

## Attack surface findings

### SCOPE — clean
- No `regrid`/`clip`/`reduce`; no new reader; `discover()` and the MS4 metadata-only
  readers reused untouched. The Scope guard restates the LOW-3 no-chunk rule.
- No transform/role/semantic/provenance field; the manifest stays exactly six fields.
- No §14 verdict, no `ValidationReport`, no `conformant` field — that is correctly
  reserved for MS6. M1/M2 hard-cut *behavior* is exercised only in the entry path, not
  as a §14 checklist verdict.
- `main.rs` untouched (MS7); no PyO3 (MS9); no exhaustive invalid family (MS8).
- The ground-truth table notes `ConsolidatedMetadataSource` is *available* but the
  `Description` struct correctly does **not** include it (architecture §3.5 omits it
  and the milestone deliverable does not list it) — proper restraint, not scope creep.

### COVERAGE — complete
Every MS5 deliverable / exit criterion / spec ref maps to a step (S1 shape, S2 verb +
§0 entry, S3 schema/golden/jsonschema/snapshot). The coverage table at the bottom of
the plan is accurate and each row is genuinely covered. The full §14 fact set becomes
discoverable through `describe` (S2). No gap found.

### ORDERING — correct
S1 (types/DTO, no IO) → S2 (verb, depends on S1 shape + MS4 discovery) → S3 (schema +
golden, depends on S2 output). Each step depends only on earlier steps. Buildable as
written.

### GREEN / COMMITTABLE — each step independently green
- S1 lands the `Description` + DTO + pure mapping with unit tests over a fixture-backed
  `Discovery`; compiles and tests without the verb. Green.
- S2 adds the boundary verb + error variant + fixture tests; green on top of S1.
- S3 adds asset + golden + tests; green on top of S2.
- Each step is one conventional commit with the mandated `./scripts/bump-version.sh
  patch` + stage `Cargo.toml` + `git tag v<version>`. No step bundles unrelated change.

### CONVENTIONS — honored
- `tracing` (`#[instrument]`, `info`/`debug`), never `println!`, for the verb.
- "No `unwrap`/`expect`/panic in library code" stated for S2; typed named-field errors,
  each doc-commented with *when* it fires.
- Enums over booleans (the existing `TimeExtentSource`/`ConsolidatedMetadataSource`
  pattern is reused, not subverted).
- Parse-don't-validate: DTOs mirror the manifest parser's two-stage discipline; domain
  types stay free of `serde::Serialize`.
- Commit messages are conventional (`feat(core): ...`).

### ACCEPTANCE QUALITY — concrete
Acceptance criteria are build/test/clippy + specific spec-check ids (M1/M2 hard cut in
entry path, §2 ordinary fields, §10 facts-only, §11 six-field floor, R4) + concrete
key-set assertions and named fixtures. Not vague.

---

## Issues filed (all low/medium — none blocking)

### [medium] S3 golden snapshot is a parsed-`Value` comparison, contradicting the
"byte-for-byte / stable JSON" R4 intent
- The S3 **intent** says `describe` of the valid fixture "equals the golden
  **byte-for-byte** (a snapshot test)", and R4 / the milestone deliverable call the
  output a *stable JSON* wire contract. But the S3 **test plan** softens the snapshot to
  *"parsed to a `serde_json::Value` equals the committed golden parsed to a `Value`
  (compare as parsed JSON so formatting differences don't make it brittle)."*
- A `Value`-equality comparison does **not** pin key ordering or pretty/compact
  formatting — exactly the surface a downstream mini-contract (MS7 CLI, MS9 PyO3) relies
  on being stable. The schema (`additionalProperties:false`) catches *extra* keys but
  not key-order or whitespace drift. As written, the snapshot would pass even if the
  serializer silently switched ordering or pretty-printing.
- **Suggested fix:** keep the `Value`-equality assert for content, AND add a string-level
  assert that `describe_json(valid)` equals the committed golden file exactly (or that
  re-serializing the parsed golden via the same `to_json_pretty` path reproduces the
  file byte-for-byte). Pin the serializer's pretty/compact + key-order choice in the S1
  DTO doc so "stable JSON" is actually locked.

### [low] S2 leaves the `DescribeError` vs `CoreError`-direct decision open, but the
milestone signature names `DescribeError`
- The MS5 deliverable (milestones.md) and the milestone-scope header specify
  `describe(path) -> Result<Description, DescribeError>`. S2 defers the choice between
  "thin `DescribeError` wrapper enum" and "return `CoreError` directly with a new
  `ManifestUnreadable` variant." Since `CoreError` is `#[non_exhaustive]`, the
  `CoreError`-direct option is technically clean, but it would contradict the named
  `DescribeError` return type in the stated signature.
- **Suggested fix:** commit S2 to introducing `DescribeError` (matching the milestone
  signature), or explicitly annotate in S2 that the milestone signature is illustrative
  and the chosen return type will be recorded; either way pick the name in the step text
  so the verb's public signature is not ambiguous at implementation time.

### [low] Top-level key naming drift `time_extent` (architecture §3.5) vs `time_extents`
(S1/S3 plan)
- Architecture §3.5 sketches the field as `time_extent` (singular); S1, the S3 schema,
  and the golden all use `time_extents` (plural) as the top-level JSON key. The plan is
  internally consistent (S1 DTO, S3 schema, golden all agree), so this does not break
  any step — but because R4 freezes the wire key set, the divergence from the
  architecture sketch should be acknowledged (the architecture is a living doc; record
  the chosen key as the canonical one).
- **Suggested fix:** add a one-line note in S1 that the canonical wire key is
  `time_extents` (and optionally fold the rename back into architecture §3.5 as an
  amendment), so a future reader does not treat the sketch's `time_extent` as the
  contract.

### [low] S3 places the schema/snapshot tests inline in `describe.rs` while the
established jsonschema pattern lives in `crates/core/tests/`
- The existing R4 manifest jsonschema test is an **integration test** at
  `crates/core/tests/manifest_schema.rs`. S3 plans the describe schema + snapshot tests
  in `crates/core/src/describe.rs (tests module)`. Both compile (`jsonschema` is a
  dev-dep visible to both), so this is not a build issue, but it diverges from the
  repo's existing convention for schema-validation tests and splits the "where do R4
  schema tests live" answer across two locations.
- **Suggested fix:** for consistency with the manifest-half precedent, prefer a
  `crates/core/tests/describe_schema.rs` integration test for the jsonschema + golden
  validation (the snapshot/Value-equality unit test may stay inline), or explicitly note
  the deliberate divergence.

---

## Conclusion

All three STEP-2 folds are genuinely and substantively incorporated. Scope, coverage,
ordering, green/committable, and conventions are clean. The issues found are all
low/medium polish items (most importantly the S3 snapshot weakening the "byte-for-byte /
stable JSON" R4 promise), none of which leave the tree red, exceed milestone scope, or
break a spec invariant. **Approved.**
