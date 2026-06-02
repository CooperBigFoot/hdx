# MS7 — The thin `hdx` CLI

> **Milestone scope (verbatim intent, milestones.md MS7).** Wrap the two verbs in a
> thin, JSON-emitting, LLM-drivable CLI (`hdx validate <path>`, `hdx describe <path>`)
> — spec §10. **No contract logic in `main.rs`**: arg-parse → call `hdx-core` →
> serialize the result to JSON on stdout → exit code. `tracing` diagnostics to stderr;
> JSON is *output*, not logging. A documented exit-code table is a deliverable.
>
> **Hard boundaries (do not cross).** No `regrid` / `clip` / `reduce`, ever. No
> inert-violating field anywhere (no transform / role / semantic / provenance, no
> derivable manifest field; the manifest stays **exactly the six floor fields**;
> `format_version` is a **hard cut**). The CLI **adds no reader and decodes no gridded
> chunk / pixel raster** — it calls the MS5/MS6 verbs and nothing else. It **must not
> re-derive or duplicate the wire shape** (the MS5 `describe.schema.json` / MS6
> `validate.schema.json` are the contracts) and **must not re-implement any §14 rule**
> in the bin (architecture §2; spec §10). The CLI introduces **no new spec MUST-checks**
> — it is glue that *exposes* the MS6 enforcement of all §14 checks through a stable
> surface.

---

## Ground truth (verified against the committed code before planning)

The CLI is **glue over an already-complete API**. Both verbs and their JSON
serialization landed in MS5/MS6; MS7 adds no contract logic. The exact `hdx-core`
surface MS7 calls (read at plan time):

| `hdx-core` entry point | Signature | What MS7 does with it |
|---|---|---|
| `hdx_core::describe::describe_json` | `fn(path: impl AsRef<Path>) -> Result<String, DescribeError>` | `hdx describe`: print the returned JSON to stdout; map `Ok`/`Err` to exit code |
| `hdx_core::validate::validate` | `fn(path: impl AsRef<Path>) -> Result<ValidationReport, ValidateError>` | `hdx validate`: read `report.conformant()` to choose exit 0 vs 1 |
| `ValidationReport::conformant` | `fn(&self) -> bool` | the 0-vs-1 discriminator |
| `ValidationReport::to_json_string` | `fn(&self) -> Result<String, serde_json::Error>` | serialize the report for stdout |
| (alt) `hdx_core::validate::validate_json` | `fn(path) -> Result<String, ValidateError>` | one-shot JSON; **but** it discards `conformant()` (the bool needed for exit 1), so MS7 prefers `validate` + `conformant()` + `to_json_string()` to derive the exit code without re-parsing JSON |

**Error → exit-code reconciliation with MS6 reality (verified):**

- `validate(path)` returns `Err` *only* for **structural / entry** failures —
  `ValidateError::ManifestUnreadable` (manifest absent/unreadable — includes a
  nonexistent dataset path: `<path>/manifest.json` is unreadable),
  `ValidateError::Manifest(CoreError::UnknownFormatVersion{..})` (the §0 hard cut),
  `ValidateError::Manifest(..)` (malformed manifest: extra/missing field, bad
  timestamp, empty crs/cadence), or `ValidateError::Discovery(..)` (an undecodable
  present artifact). **These map to exit 2.**
- A **violated `MUST` that ran** is **never an `Err`**: it is `Ok(report)` with
  `report.conformant() == false`. **This maps to exit 1** — *distinct* from the exit-2
  error cases.
- `describe_json(path)` returns `Err(DescribeError::*)` for the same structural/entry
  failures (its hard cut is `DescribeError::Manifest(CoreError::UnknownFormatVersion)`),
  which map to exit 2. A successful `describe` maps to exit 0.

This was confirmed against the committed MS6 tests: `validate` over
`conformance/invalid/missing-root-rollup/` returns `Ok(report)` with `conformant:false`
(L1 fails) → **exit 1**; `validate`/`describe` over
`conformance/invalid/wrong-format-version/` returns `Err(..UnknownFormatVersion)` →
**exit 2**; a nonexistent path → `Err(ManifestUnreadable)` → **exit 2**.

**Starting state of the bin (read at plan time).**

- `src/main.rs` is the placeholder: it inits `tracing_subscriber::fmt()` and logs
  `info!("hello")`. No subcommands, no `clap`.
- Root `Cargo.toml` (`[package] name = "hdx"`) already depends on `hdx-core` (path),
  `anyhow = "1"`, `tracing = "0.1"`, `tracing-subscriber = "0.3" (env-filter)`. **No
  `clap`, no `[dev-dependencies]`, no `tests/` dir.**
- Fixtures exist at `conformance/valid/minimal/`,
  `conformance/invalid/missing-root-rollup/`, `conformance/invalid/wrong-format-version/`.
- Committed schemas exist at `schemas/describe.schema.json` and
  `schemas/validate.schema.json`.

---

## Ordering rationale

MS7 is the smallest milestone: one binary, two subcommands, three exit codes. It is
sequenced bottom-up so the repo is **green after every step** and each commit is a
coherent reviewable unit:

1. **S1 — arg surface + serialization, no exit-code logic yet.** Bring in `clap`, give
   `main.rs` the `describe`/`validate` subcommands that parse a path, call the verb,
   print JSON to stdout, and route `tracing` to stderr. At this step every path
   (success or error) returns the process's default exit status; the *value* delivered
   is the JSON-emitting surface itself. Doing the arg-parse + serialize wiring first
   isolates the dependency add (`clap`) and the glue shape from the exit-code semantics,
   so the diff is small and the "thin glue only" property is reviewable in one place.
   This must land first because S2's exit-code mapping has nothing to map without the
   verbs being called.

2. **S2 — the exit-code table (0 / 1 / 2), implemented + asserted.** Replace the
   default exit status with the explicit `ExitCode` mapping: 0 = describe ok OR
   validate `conformant:true`; 1 = validate `conformant:false`; 2 = any
   structural/entry error (bad args, unreadable/nonexistent path, malformed manifest,
   §0 hard cut). This is the load-bearing, scripting-facing contract, so it gets its
   own commit with the CLI integration tests that assert **each** code against the
   committed fixtures. It depends on S1 (the verbs must already be called).

3. **S3 — schema-conformance + the documented exit-code table.** Add CLI integration
   tests asserting that `hdx describe`'s stdout validates against
   `schemas/describe.schema.json` and `hdx validate`'s stdout validates against
   `schemas/validate.schema.json` (reusing the `jsonschema` dev-dep at the bin-test
   level), and fold the exit-code table + the "thin glue / no contract logic" rule into
   a short `README`/doc surface for the bin. This keeps the CLI a faithful, LLM-drivable
   surface over the verbs and pins the wire shape end-to-end through the process
   boundary. It comes last because it asserts properties of the output produced by S1
   and the codes fixed by S2.

Each step is independently committable, leaves `cargo build` + `cargo test` +
`cargo clippy --all-targets -- -D warnings` green, and is a meaningful unit of progress.
S1 = "the CLI exists and emits JSON"; S2 = "the CLI's exit codes are a contract"; S3 =
"the CLI's output is schema-faithful and documented".

---

## Scope guard

No step in MS7 does any later milestone's work and none touches `hdx-core` contract
logic:

- **No MS8 work.** MS7 uses only the three already-committed fixtures
  (`valid/minimal`, `invalid/missing-root-rollup`, `invalid/wrong-format-version`). It
  **does not** add the exhaustive one-violation-per-check invalid family or golden
  `describe`/`validate` outputs — that is MS8.
- **No MS9 work.** No `crates/python`, no PyO3, no maturin.
- **No contract logic in `main.rs`.** Every step keeps the bin to arg-parse → call the
  `hdx-core` verb → serialize the returned `Description`/`ValidationReport` to JSON →
  map to an exit code. No §14 rule, no manifest parsing, no reader, no discovery is
  re-implemented in the bin; the wire shape is **reused** from the MS5/MS6 serializers,
  never re-derived.
- **No new readers, no chunk/pixel decode.** The CLI calls the verbs and decodes
  nothing itself (LOW-3 holds transitively).
- **Inert / agnostic discipline preserved.** MS7 adds **no** type or field anywhere
  carrying transform / role / semantic / provenance. The manifest stays exactly the six
  floor fields (MS7 never constructs or mutates a manifest). `format_version` remains a
  hard cut — surfaced by the verbs as an exit-2 error, never softened by the CLI.
- **No spec drift.** MS7 introduces no new spec MUST-check; it exposes MS6's
  enforcement of the full §14 list (M1–M6, L1–L3, I1–I3, H1–H2, T1–T2, G1–G3, Geo1)
  through a stable interface. The spec and architecture files are not modified.
- **Diagnostics vs output split.** Diagnostics go through `tracing` to **stderr**; the
  JSON on **stdout** is OUTPUT, not logging — never `println!` for diagnostics
  (CLAUDE.md / architecture §2).

---

## MS7-S1 — `clap` subcommands: arg-parse → call verb → JSON to stdout

**id:** MS7-S1

**intent.** Stand up the thin CLI surface: `hdx describe <path>` and
`hdx validate <path>`. Each subcommand parses exactly one dataset path, calls the
corresponding `hdx-core` verb, prints the returned JSON to **stdout**, and keeps all
diagnostics on **stderr** via `tracing`. No exit-code semantics yet (every path returns
the default success status; errors surface via `anyhow` to stderr). This isolates the
`clap` dependency add and the glue shape into one reviewable commit and establishes the
"thin glue only" boundary before exit codes are layered on. Independently committable:
the workspace builds, the new bin runs, and the existing `hdx-core` tests are
untouched, so the repo stays green.

**changes.**
- `Cargo.toml` (root `hdx` bin): add `clap = { version = "4", features = ["derive"] }`.
  Patch-bump the version per CLAUDE.md and stage `Cargo.toml`.
- `src/main.rs`: replace the `info!("hello")` placeholder with a `clap`-derived
  `Cli` + `Command { Describe { path }, Validate { path } }`. Init `tracing_subscriber`
  writing to **stderr** (`.with_writer(std::io::stderr)`), keep the env-filter. Dispatch:
  - `Describe { path }` → `hdx_core::describe::describe_json(&path)` → print the JSON
    string to stdout (a single `println!` of the JSON *value* is the program's output;
    diagnostics stay on `tracing`/stderr — the JSON is output, not a diagnostic).
  - `Validate { path }` → `hdx_core::validate::validate(&path)?` then
    `report.to_json_string()` → print to stdout. (Using `validate` + `to_json_string`
    rather than `validate_json` so S2 can read `conformant()` for the exit code without
    re-parsing JSON.)
  - Both arms use `anyhow` with `.context()` for enriched errors; in S1 an `Err` simply
    propagates out of `main() -> anyhow::Result<()>` (default failure status). Use
    `.expect("reason")` only for the truly-unrecoverable subscriber init, per CLAUDE.md.
- Module-level `//!` doc on `main.rs` stating the bin is **thin glue only** (no contract
  logic; JSON to stdout, tracing to stderr) — proportional docs (architecture §2).

**test_plan.**
- A `tests/cli.rs` integration test (bin test) is NOT required to assert exit codes yet
  (that is S2), but S1 adds a minimal smoke test in `tests/cli.rs`: invoke the built
  binary (`env!("CARGO_BIN_EXE_hdx")`) with `describe conformance/valid/minimal` and
  assert the process produced **non-empty stdout that parses as a JSON object** (using
  `serde_json` as a bin dev-dep). This proves the surface emits JSON without yet pinning
  the exit code or schema.
- A second smoke test: `validate conformance/valid/minimal` produces stdout that parses
  as a JSON object containing a `conformant` key (the report shape), confirming the
  validate arm is wired to the report serializer.
- Tests resolve the fixture dir from `CARGO_MANIFEST_DIR` (the bin crate root) joined
  with `conformance/...` (the workspace root is the bin crate root for the `hdx`
  package).
- Add `serde_json` (and `assert_cmd`-style invocation via `std::process::Command` on
  `CARGO_BIN_EXE_hdx`, no extra crate needed) under the bin's `[dev-dependencies]`.

**acceptance.**
- `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings` all pass.
- `hdx describe conformance/valid/minimal` and `hdx validate conformance/valid/minimal`
  each print a JSON object to **stdout**; no diagnostic text contaminates stdout
  (diagnostics, if any, are on stderr via `tracing`).
- `main.rs` contains **only** arg-parse, the verb call, JSON serialization, and (in S1)
  default error propagation — no §14 rule, no manifest parse, no reader (review gate;
  advances the MS7 exit criterion "no contract logic in `main.rs`").
- No `println!` used for diagnostics (only for the JSON output value); diagnostics go
  through `tracing` to stderr.
- No new spec MUST-check (CLI is glue); MS7 exposes the existing MS5/MS6 verb behavior.
- Commit via `./scripts/bump-version.sh patch` → stage `Cargo.toml` + code → conventional
  commit → `git tag v<version>`.

**spec_refs.** §10 (thin JSON-emitting, LLM-drivable CLI wrapping the verbs);
architecture §2 (thin glue; JSON to stdout, `tracing` to stderr; never `println!` for
diagnostics); R4 (reuse the MS5/MS6 stable JSON shape, do not re-derive).

**commit_message.** `feat(cli): add hdx describe/validate subcommands emitting verb JSON to stdout`

---

## MS7-S2 — the exit-code table (0 / 1 / 2), implemented and asserted

**id:** MS7-S2

**intent.** Make the CLI a scripting-grade, LLM-drivable surface by implementing the
documented exit-code contract and asserting each code with integration tests against the
committed fixtures. This is the load-bearing reconciliation of the exit codes with MS6
reality: a `conformant:false` **report** (exit 1) is distinct from a structural/entry
**error** (exit 2), and the §0 hard cut / malformed / unreadable / nonexistent-path
cases all surface as `Err` from the verb and map to exit 2 — **never** to a
`conformant:false` report. Independently committable: it changes only the bin's
result→exit mapping and its tests, leaving `hdx-core` and the repo green.

**Exit-code table (the deliverable):**

| Code | Meaning |
|---|---|
| `0` | success — `describe` succeeded, **or** `validate` returned `conformant: true` |
| `1` | non-conformant — `validate` returned a report with `conformant: false` |
| `2` | usage / IO error — bad args, unreadable / nonexistent path, **malformed** manifest, or the §0 hard cut (unknown `format_version`) |

**changes.**
- `src/main.rs`: change `main` to return `std::process::ExitCode` (or `fn main()` that
  computes and returns an `ExitCode`), and implement the mapping in the dispatcher:
  - `describe`: on `Ok(json)` → print JSON to stdout, return `ExitCode::SUCCESS` (0);
    on `Err(DescribeError::*)` → log the error via `tracing::error!` to stderr (with
    `anyhow` `.context`) and return `ExitCode::from(2)`.
  - `validate`: on `Ok(report)` → print `report.to_json_string()` to stdout, then return
    `ExitCode::SUCCESS` (0) if `report.conformant()` else `ExitCode::from(1)`; on
    `Err(ValidateError::*)` → log to stderr and return `ExitCode::from(2)`.
  - `clap` arg-parse failure (bad/missing args) → `clap` already exits with its usage
    error; ensure that maps to exit 2 (configure `clap`'s error exit code, or let the
    derive `Parser::parse()` exit — assert the observed code is 2 in a test; if `clap`'s
    default is 2 this is satisfied directly).
  - The §0 hard cut (`...Manifest(CoreError::UnknownFormatVersion{..})`) is **not**
    special-cased: it is one of the `Err` variants and falls into the exit-2 arm — the
    CLI never softens it into a report. This is asserted by a test.
- The mapping is a small, total `match` on the verb's `Result` — **no contract logic**,
  only result→code routing (review gate).

**test_plan.** Extend `tests/cli.rs` (invoking `CARGO_BIN_EXE_hdx` via
`std::process::Command`, asserting `output.status.code()`):
- `describe conformance/valid/minimal` → exit **0**, stdout is a JSON object.
- `validate conformance/valid/minimal` → exit **0** (the fixture is conformant), stdout
  is a JSON object with `"conformant": true`.
- `validate conformance/invalid/missing-root-rollup` → exit **1**, stdout is a JSON
  object with `"conformant": false` (the L1 fail is carried in the report, **not** as an
  error) — this asserts the exit-1 ≠ exit-2 distinction.
- `validate conformance/invalid/wrong-format-version` → exit **2** (the §0 hard cut is a
  verb `Err`, never a `conformant:false` report); stdout carries **no** report JSON (or
  is empty); the diagnostic is on stderr.
- `describe conformance/invalid/wrong-format-version` → exit **2** (hard cut applies to
  `describe` too).
- `validate <nonexistent path>` and `describe <nonexistent path>` → exit **2**
  (`ManifestUnreadable`); diagnostic on stderr.
- Bad args (e.g. no subcommand, or `validate` with no path) → exit **2** (usage error);
  assert the code.

**acceptance.**
- `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings` all pass.
- Each of the three exit codes is implemented and **asserted by a CLI test** against the
  committed fixtures, including the load-bearing distinction: exit 1 (a
  `conformant:false` report) vs exit 2 (a structural/entry `Err`, including the §0 hard
  cut, malformed manifest, unreadable/nonexistent path).
- `validate` on `missing-root-rollup` is observed as `Ok`→exit 1 (not an error); both
  wrong-version fixtures and the nonexistent path are observed as `Err`→exit 2.
- No contract logic added to `main.rs` (only result→exit-code routing); JSON stays on
  stdout, diagnostics on stderr (never `println!` for diagnostics).
- No new spec MUST-check; the CLI exposes MS6's enforcement of all §14 checks through a
  stable interface with a stable exit-code contract.
- Commit via `./scripts/bump-version.sh patch` → stage `Cargo.toml` + code → conventional
  commit → `git tag v<version>`.

**spec_refs.** §10 (the verbs surfaced as an LLM-drivable CLI); §0 (the hard cut surfaces
as an error, never a softened verdict — exit 2); §14 (a violated MUST is a
`conformant:false` report — exit 1, distinct from an error); architecture §2 (thin glue,
exit code from the verb result); R4 (stable JSON shape preserved through the process).

**commit_message.** `feat(cli): map verb results to the 0/1/2 exit-code contract`

---

## MS7-S3 — schema-conformant stdout + documented exit-code table

**id:** MS7-S3

**intent.** Lock the CLI as a *faithful* surface over the verbs: assert end-to-end
(through the process boundary) that `hdx describe`'s stdout validates against
`schemas/describe.schema.json` and `hdx validate`'s stdout validates against
`schemas/validate.schema.json`, reusing the same `jsonschema` mechanism the
`hdx-core` tests use (now as a **bin** dev-dependency). Then fold the exit-code table and
the "thin glue / no contract logic / JSON-to-stdout, tracing-to-stderr" rule into a short
`README.md` for the bin so the documented contract lives next to the binary. This proves
the CLI does not silently reshape the wire contract the verbs define, and gives an
LLM/operator a single documented reference. Independently committable: it adds tests +
docs only, leaving runtime behavior unchanged and the repo green.

**changes.**
- `Cargo.toml` (root `hdx` bin): add `jsonschema` (matching the version pinned in
  `crates/core` dev-deps, currently `0.46`) and `serde_json` under the bin's
  `[dev-dependencies]` (if not already present from S1). Patch-bump + stage `Cargo.toml`.
- `tests/cli.rs`: add schema-conformance tests that
  - run `hdx describe conformance/valid/minimal`, parse stdout as JSON, compile
    `schemas/describe.schema.json` with `jsonschema`, and assert the stdout **validates**;
  - run `hdx validate conformance/valid/minimal`, parse stdout, compile
    `schemas/validate.schema.json`, and assert the stdout **validates**;
  - run `hdx validate conformance/invalid/missing-root-rollup` (exit 1) and assert its
    stdout (the `conformant:false` report) **also** validates against
    `schemas/validate.schema.json` — proving a non-conformant report is still
    schema-faithful.
  - schema files are resolved from `CARGO_MANIFEST_DIR` joined with `schemas/...`.
- `README.md` (a new short bin-level doc, e.g. at the repo root or `src/`-adjacent as the
  `hdx` package README) documenting: the two subcommands, the exit-code table (0/1/2 with
  the exit-1≠exit-2 distinction and the §0 hard cut → exit 2 note), and the invariants
  (thin glue only — no contract logic; JSON on stdout is output; diagnostics via
  `tracing` to stderr). Reference `hdx-core` as the home of all contract logic
  (architecture §2). Keep it proportional — a CLI reference, not a re-statement of the
  spec.

**test_plan.**
- `describe_stdout_validates_against_describe_schema`: stdout of
  `describe conformance/valid/minimal` validates against `schemas/describe.schema.json`.
- `validate_stdout_validates_against_validate_schema`: stdout of
  `validate conformance/valid/minimal` validates against `schemas/validate.schema.json`.
- `nonconformant_validate_stdout_still_validates_against_schema`: stdout of
  `validate conformance/invalid/missing-root-rollup` (exit 1) validates against
  `schemas/validate.schema.json` (a `conformant:false` report is still a valid report).
- All schema-conformance tests assert the schema compiles and the parsed stdout is valid
  per `jsonschema`; failures panic with the validation error for debuggability.

**acceptance.**
- `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings` all pass.
- `hdx describe` stdout validates against `schemas/describe.schema.json` and
  `hdx validate` stdout (both the conformant and the `conformant:false` cases) validates
  against `schemas/validate.schema.json`, asserted via the `jsonschema` dev-dep at the
  bin-test level — the CLI is a faithful, non-reshaping surface over the verbs (reuses
  the MS5/MS6 wire shape, never re-derives it).
- The exit-code table and the thin-glue / stdout-vs-stderr invariants are documented in
  a bin-level `README.md`.
- No contract logic added to `main.rs` (docs + tests only); no new spec MUST-check.
- Commit via `./scripts/bump-version.sh patch` → stage `Cargo.toml` + code/docs →
  conventional commit → `git tag v<version>`.

**spec_refs.** §10 (the CLI is a faithful LLM-drivable surface over the verbs);
architecture §2 (thin glue; reuse the serialized wire shape; document the contract); R4
(the `describe`/`validate` JSON schemas are the mini-contract the CLI must honor
verbatim).

**commit_message.** `test(cli): assert stdout matches describe/validate schemas; document exit codes`

---

## Coverage check — every MS7 deliverable, exit criterion, and fold-in is assigned

| MS7 deliverable / exit criterion (milestones.md) | Step |
|---|---|
| `src/main.rs` gains `validate` + `describe` subcommands (e.g. via `clap`), each taking a dataset path | S1 |
| `hdx describe <path>` prints the `Description` JSON (MS5 schema) to stdout | S1 (emit) + S3 (schema-asserted) |
| `hdx validate <path>` prints the `ValidationReport` JSON (MS6) to stdout | S1 (emit) + S3 (schema-asserted) |
| Exit-code table 0 / 1 / 2 implemented | S2 |
| Each exit code asserted by a CLI test (0 success/conformant, 1 non-conformant, 2 usage/IO) | S2 |
| `anyhow` `.context()` in the glue; `tracing` to stderr | S1 (wiring) + S2 (error logging) |
| CLI tests assert stdout is valid JSON matching the relevant schema | S3 |
| No contract logic in `main.rs` (arg-parse, call, serialize, exit) — review gate | S1, S2, S3 (held every step) |
| JSON on stdout; `tracing` diagnostics on stderr; never `println!` for diagnostics | S1, S2 |
| No new spec MUST-check; CLI exposes MS6 enforcement of all §14 checks | S1, S2, S3 (held) |
| Commit via bump+tag convention | S1, S2, S3 (each) |

### Fold-in coverage

| Folded critique issue | Where addressed |
|---|---|
| Exit-code mapping reconciled with MS6: 0/1/2; `Err` (hard cut / unreadable / malformed) → 2, distinct from `conformant:false` report → 1; tests assert valid+missing-rollup→1, wrong-version→2, nonexistent→2 | S2 (full assertion matrix incl. exit-1 vs exit-2 distinction and the §0 hard cut → exit 2) |
| Thin glue ONLY (architecture §2; spec §10): no contract logic / no re-derived wire shape / no re-implemented §14 rule in the bin; diagnostics via `tracing` to stderr, JSON on stdout is output | S1 (glue shape + stderr/stdout split), held in S2 and S3; reuses MS5/MS6 serializers, never re-derives |
| Schema-conformant output: `hdx describe` stdout matches `schemas/describe.schema.json`, `hdx validate` stdout matches `schemas/validate.schema.json` (reuse `jsonschema` at the bin-test level) | S3 |
