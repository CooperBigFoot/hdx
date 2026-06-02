# Adversarial critique — MS1 STEP PLAN (`planning/undefined/steps.md`)

> **Milestone under review:** MS1 — Core types + manifest parse + manifest JSON
> Schema. The planning directory is named `undefined/` (the orchestration
> milestone variable did not resolve to `MS1`), but the file content is the MS1
> step plan and is **byte-identical** to `planning/MS1/steps.md`
> (`diff` produced no output). This critique treats it as the MS1 plan.
>
> **Reviewed against:** `spec/HDX_SPEC.md` (canonical), `architecture.md` §2/§3/§6/§7,
> `planning/milestones.md` (MS1 section, lines 150–229), and the folded critique
> `planning/milestones-critique.md` **MED-3**.
>
> **Verdict: APPROVED.** Zero high/critical issues. Full deliverable + exit-criterion
> + spec-ref coverage. Correct bottom-up ordering. Every step independently
> committable and green. Conventions honored. MED-3 genuinely folded into both the
> parser (S4) and the schema (S5). Only five LOW issues, all cosmetic/clarity.

---

## 1. Scope discipline — PASS

The Scope guard (steps.md §"Scope guard", lines 18–48) is explicit and is honored
by every step:

- **No external IO.** S1–S3 are pure types. S4 parses a JSON *string*. S5's test
  reads a committed schema asset + an in-test JSON literal (a unit-test asset
  read, not dataset IO). No filesystem walk, no parquet/Zarr/COG/geoparquet reads
  anywhere — those are correctly deferred to MS3/MS4.
- **No verb logic.** No `describe` (MS5), no `validate` (MS6), no §14 rule engine.
  S2/S4/S5 enforce only the manifest-boundary foundations (M1–M4); H1 is *typed*
  in S3 but explicitly *not enforced* (deferred to MS6). No cross-file / cross-basin
  rule (M5, M6, L*, I*, H2, T*, G*, Geo1) is touched.
- **No CLI changes** (`src/main.rs` untouched — MS7).
- **No reader crates.** S4 adds only `serde`/`serde_json` + one RFC 3339 time
  crate; S5 adds `jsonschema` as a **dev-dependency** only. No `parquet`/`arrow`/
  `zarrs`/`tiff`/`geoarrow` — correctly held for MS3/MS4.
- **Inert/agnostic (spec §1/§13) — verified field-by-field.** No type or field
  carries transform / normalization / role / semantic-type / reduction /
  provenance / computation-source. `Manifest` is exactly the six floor fields
  (§11) with no derivable field (no content hash, no data-version, no field
  catalog, no basin list). `Field` carries only `name`/`quadrant`/`dtype`/`units`/
  `grid_label` (architecture §3.3). `Dtype` is semantics-opaque (no
  continuous/categorical, and a `reject-don't-carry` policy with **no**
  `Other(String)`). `Units` is an opaque optional string. The newtypes are opaque
  producer strings. This is exactly the spec §1 discipline.
- **`format_version` HARD cut.** S2 models a single-arm enum (`V0_1`); parse
  succeeds only on the exact string `"0.1"` and errors otherwise. No multi-version
  path is representable (spec §0.2).

**No step does a later milestone's work and none violates the inert/agnostic
discipline.** No regrid/clip/reduce/hydrology anywhere (spec §10 — excluded
forever).

---

## 2. Coverage — PASS (every MS1 deliverable + exit criterion + spec ref assigned)

Cross-walk of `milestones.md` MS1 deliverables/exit-criteria against the steps:

| MS1 deliverable / exit criterion (milestones.md 159–214) | Covered by | OK |
|---|---|---|
| 8 opaque newtypes (`BasinId`…`ProducerVersion`), constructed at boundary | S1 | ✓ |
| `FormatVersion` enum `V0_1` only; parse `"0.1"`→Ok, else `UnknownFormatVersion` (M2 hard cut) | S2 | ✓ |
| `Temporal`/`Shape`/`Quadrant` enums (enums over booleans, §2) | S3 | ✓ |
| `Field` with `grid_label.is_some() ⇔ Shape::Gridded` in constructor | S3 | ✓ |
| Closed `Dtype` + fallible `parse_dtype` (no panic, no default, `UnknownDtype`) | S3 | ✓ |
| `Units` (opaque optional, no parse) | S3 | ✓ |
| `Manifest` six fields; `deny_unknown_fields`; RFC 3339 `created_at`; non-empty `crs`/`cadence`; `format_version` read+cut first (M1–M4) | S4 | ✓ |
| `thiserror` enum, named-field, doc-commented variants + later-MS stubs | S1 (+ `MismatchedGridLabel` in S3) | ✓ |
| `schemas/manifest.schema.json` (`additionalProperties:false`, six required, types) | S5 | ✓ |
| `jsonschema` dev-dep + test: §11 example validates; 7-field rejected by **both** parser and schema | S5 | ✓ |
| `crates/core/README.md` (Mermaid module map + glossary) | S6 | ✓ |
| `cargo build`+`test`+`clippy -D warnings` green after each step | S1–S6 acceptance | ✓ |
| Bump+tag commit discipline | S1–S6 acceptance | ✓ |

**Spec-check coverage:** M1 (S4 read-first + valid-JSON), M2 (S2 type-level + S4
applied-first), M3 **both directions** (S4 parser + S5 schema), M4 (S4 + S5). H1 is
*typed scaffolding only* (S3) — correctly deferred for enforcement to MS6, matching
the milestones.md exit criterion. All other §14 checks (M5, M6, L*, I*, H2, T*, G*,
Geo1) are out of MS1 scope (no IO) and explicitly deferred. **No coverage gap.**

**Spec refs** cited per step (§0/§1/§2/§11/§14 M1–M4 + architecture §3.1–§3.4/§3.6/§7)
match the milestones.md MS1 spec-refs (line 216–218).

---

## 3. MED-3 fold — PASS (genuinely incorporated, both directions, both mechanisms)

The folded critique MED-3 requires proving **both** the "too-many" *and* the
"too-few" manifest-field rejection, in **both** the parser and the schema:

- **Parser (S4):** explicit `MissingManifestField { field }` mapped from serde's
  missing-field error (too-few), alongside `deny_unknown_fields` → `ExtraManifestField`
  (too-many). Test plan asserts a 5-field manifest (omitting `cadence`) →
  `MissingManifestField { field: "cadence" }` *and* a 7-field manifest →
  `ExtraManifestField` (steps.md lines 311–312).
- **Schema (S5):** `required` lists **all six** floor fields (rejects too-few) +
  `additionalProperties:false` (rejects too-many); the `jsonschema` test asserts a
  5-field manifest fails schema **and** parser, and a 7-field manifest fails schema
  **and** parser (steps.md lines 364–367). This is exactly MED-3's suggested fix
  (add the missing-field negative to the schema test; schema `required` lists all
  six). **Genuinely folded, not merely name-checked.**

The cross-cutting confirmations (inert/agnostic, hard-cut-first, six-field floor,
no derivable field) are all carried in the Scope guard and re-asserted in each
step's acceptance.

---

## 4. Green / committable — PASS

Each step is one conventional commit and leaves `cargo build` + `cargo test` +
`cargo clippy --all-targets -- -D warnings` green:

- **S1** adds compiling, tested newtypes + the `CoreError` enum and wires only the
  modules whose files it creates. The stub error variants are members of a `pub`
  enum reachable from `lib.rs`, so Rust does **not** emit `dead_code` for them
  (public API items are not dead). The test plan additionally constructs each
  variant, so even the conservative `#[allow(dead_code)]` hedge is unnecessary —
  either way clippy stays green. No prior step is depended on (it is the floor).
- **S2** depends only on S1's `CoreError`. Self-contained type + parse + tests.
- **S3** depends only on S1 (`FieldName`/`GridLabel`/`CoreError`); adds
  `MismatchedGridLabel` to the enum (still green). No later type depends on it.
- **S4** depends on S1 + S2; introduces `serde`/`serde_json` + one RFC 3339 crate
  (pinned, recorded). The DTO + boundary parse + tests compile and pass standalone.
- **S5** depends on S4 (schema↔parser agreement); adds a dev-dep + asset + test —
  no production behavior change.
- **S6** is docs-only; cannot break the gate; placed last so the module map is final.

No step leaves the tree red, and no step bundles unrelated changes. The
`--all-targets` clippy in the steps is **stricter** than the bare
`cargo clippy -- -D warnings` in the milestones.md exit criterion — an
improvement, not a regression.

The pre-existing scaffold supports this: `crates/core/Cargo.toml` already carries
`thiserror = "2"` and `tracing = "0.1"` (so S1 needs no dep change), `lib.rs` is
the one-line stub the plan replaces, and `scripts/bump-version.sh` exists for the
mandated bump+tag.

---

## 5. Ordering — PASS

Strict type-dependency, bottom-up: S1 (newtypes+errors) → S2 (`FormatVersion`,
needs `CoreError`) → S3 (field 2×2, needs newtypes+`CoreError`) → S4 (`Manifest`,
needs `FormatVersion`+newtypes) → S5 (schema test, needs `Manifest`) → S6 (docs,
needs all modules to exist). **No step depends on a later step.** The sequence is
buildable as written; each commit compiles.

---

## 6. Conventions — PASS

- **`tracing`, never `println!`** — S2/S4 explicitly route reject-path diagnostics
  through `debug!`/`warn!`; `#[instrument(skip(json))]` on the public parse. No
  `println!` anywhere.
- **No `unwrap`/`expect`/`panic` in library code** — affirmatively forbidden in
  S1/S3/S4 acceptance; the `Dtype` parse and `Field::new` are fallible-typed; the
  `Dtype` unknown path is asserted via `Result`, not `#[should_panic]`.
- **`thiserror`, named fields, doc-comments** — `CoreError` variants are named-field
  (no tuple variants) and doc-commented with *when* they fire, including the
  later-MS stubs documented as intentional skeletons.
- **Enums over booleans** — `Temporal`/`Shape`/`Quadrant` are enums; the plan
  explicitly bans a `bool` for the field axes.
- **Parse, don't validate / newtypes** — raw JSON/strings are parsed into domain
  types at the boundary (`from_json`, `parse_dtype`, `FormatVersion::from_str`);
  internal types are valid-by-construction.
- **`deny_unknown_fields`** — present on the manifest DTO (M3 too-many).
- **No `use super::*`** — not present; explicit imports implied.
- **Docs** — S6 produces a crate README with a **Mermaid** module map (not ASCII)
  + glossary, per CLAUDE.md's complex-crate convention.
- **Commit discipline** — each step ends with `./scripts/bump-version.sh patch` +
  stage `Cargo.toml` + conventional commit + `git tag v<version>`.
- **Commit messages** are conventional: `feat(core): …` (S1–S4),
  `test(core): …` (S5), `docs(core): …` (S6).

---

## 7. Acceptance quality — PASS

Acceptance criteria are concrete: each step names `cargo build`/`test`/
`clippy --all-targets -- -D warnings` plus the specific spec-check ids it advances
(M1/M2 in S2; M1–M4 in S4; M1–M4 schema mirror in S5; H1 *typed only* in S3). The
test plans enumerate exact inputs (e.g. `"0.10" ≠ "0.1"` exact-string match;
date-only `created_at` → `InvalidTimestamp`; 5-field vs 7-field manifests).

---

## 8. Issues filed (all LOW — none block approval)

### LOW-1 — S1 `lib.rs` module-declaration wording is self-contradictory (clarity)
The S1 Changes bullet (steps.md lines 104–107) first lists
`pub mod format_version; pub mod field; pub mod manifest;` then says
"S1 declares `newtypes` + `error`." Read literally, the first clause would declare
modules whose files do not yet exist → a red build. The controlling final clause
("S1 declares `newtypes` + `error`") resolves it correctly, but the bullet should
state unambiguously that S1 declares **only** `pub mod newtypes; pub mod error;`
and that the other three are added by their own steps.
**Fix:** rewrite the bullet so the first clause is clearly the eventual tree, not
S1's declarations.

### LOW-2 — `bool` named field in `MismatchedGridLabel { quadrant, has_label }` (convention, borderline)
S3 adds `MismatchedGridLabel { quadrant, has_label: bool }` to `CoreError`
(steps.md line 232). The "enums over booleans" rule targets *domain-state*
parameters; here `has_label` is diagnostic display payload inside an error variant,
which is acceptable. Still, it is the one `bool` in the plan and could be phrased
to avoid any ambiguity (e.g. encode the offending combination as the `Quadrant`
plus the presence as part of the message), or simply note it is display-only.
**Fix:** keep as-is but document it as diagnostic payload, or drop `has_label` in
favor of a message that states the mismatch.

### LOW-3 — `DtypeError` (milestones.md) vs `CoreError::UnknownDtype` (steps) naming (spec-drift, cosmetic)
`milestones.md` line 170 illustrates `parse_dtype(&str) -> Result<Dtype, DtypeError>`,
while S3 uses `Result<Dtype, CoreError>` with an `UnknownDtype` variant. The
milestones.md *error-enum-skeleton* deliverable (line 183) lists `UnknownDtype` as a
single-enum variant, so the steps' choice is consistent with the deliverable; the
`DtypeError` mention was illustrative. Harmless, but worth a one-line note so a
reader does not expect a separate `DtypeError` type.
**Fix:** none required; optionally note "single `CoreError` enum, `UnknownDtype`
variant" in the step.

### LOW-4 — S1 acceptance "advances spec-check foundations only" is slightly soft (acceptance quality)
S1 enforces no spec check (correct), but its acceptance phrasing
("Advances spec-check foundations only by establishing the typed vocabulary")
is less crisp than the concrete spec-check ids in S2/S4/S5. Acceptable because the
concrete enforcement is carried by later steps; flagged only for symmetry.
**Fix:** state plainly "S1 enforces no spec check; it establishes the type
vocabulary that S2/S4 enforce against."

### LOW-5 — Planning directory mislabeled `planning/undefined/` (process, not plan content)
The step plan lives in `planning/undefined/` rather than `planning/MS1/`; the
content is byte-identical to `planning/MS1/steps.md`. The orchestration milestone
variable evidently resolved to the literal string `undefined`. This is an
orchestration/process artifact, not a defect in the plan's content, but it should
be corrected so the canonical location is `planning/MS1/`.
**Fix:** ensure the milestone id resolves to `MS1` so the plan, critique, and
versioned copies land under `planning/MS1/`.

---

## 9. Bottom line

The plan is correct, complete, conservative, and green at every step. It folds
MED-3 in both directions across both the parser and the schema, honors the
inert/agnostic discipline and the six-field floor down to the field level, models
the hard version cut as a single-arm enum, and sequences strictly bottom-up. The
only findings are five LOW clarity/cosmetic items, none of which affect
buildability, coverage, scope, or conventions.

**APPROVED.**
