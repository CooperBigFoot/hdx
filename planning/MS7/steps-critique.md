# MS7 Step-Plan Critique — adversarial review

**Verdict: APPROVED** (severity: low)

Reviewed: `planning/MS7/steps.md` against `spec/HDX_SPEC.md`, `architecture.md`, and
the MS7 section of `planning/milestones.md`. All ground-truth claims in the plan were
verified against the committed `hdx-core` code, fixtures, and schemas before judging.

---

## Ground-truth verification (the load-bearing claims hold)

Every API/behavioral claim the plan stands on was checked against committed code:

| Plan claim | Verified against | Result |
|---|---|---|
| `hdx_core::describe::describe_json(path) -> Result<String, DescribeError>` | `crates/core/src/describe.rs:355` | exact match |
| `hdx_core::validate::validate(path) -> Result<ValidationReport, ValidateError>` | `crates/core/src/validate.rs:1260` | exact match |
| `ValidationReport::conformant(&self) -> bool` | `validate.rs:383` | exact match |
| `ValidationReport::to_json_string(&self) -> Result<String, serde_json::Error>` | `validate.rs:411` | exact match |
| `validate_json` discards `conformant()` (so S1/S2 prefer `validate` + `to_json_string`) | `validate.rs:1308` | confirmed — wrapper returns only the JSON string |
| `ValidateError` variants: `ManifestUnreadable`, `Manifest(CoreError)`, `Discovery(CoreError)`, `Serialize` (`#[non_exhaustive]`) | `error.rs:380` | exact match |
| §0 hard cut surfaces as `Err(Manifest(UnknownFormatVersion))`, BEFORE discovery | `validate.rs:1275` (early `?` precedes `discover`) | confirmed |
| `missing-root-rollup` → `Ok(report)` with `conformant:false` (exit 1, NOT an Err) | committed MS6 test `missing_root_rollup_pins_exactly_l1_and_is_non_conformant` (`validate.rs:1950`) and `validate.rs:2311` ("the gap is a check fail, not an Err") | confirmed — the exit-1≠exit-2 distinction is real |
| `wrong-format-version` manifest is `format_version: "0.2"` | `conformance/invalid/wrong-format-version/manifest.json` | confirmed → hard-cut Err → exit 2 |
| `src/main.rs` is the `info!("hello")` placeholder; root `Cargo.toml` has `hdx-core`/`anyhow`/`tracing`/`tracing-subscriber`, no `clap`, no dev-deps, no `tests/` | `src/main.rs`, `Cargo.toml`, `ls` | confirmed exactly |
| Schemas exist: `schemas/describe.schema.json`, `schemas/validate.schema.json` | `ls schemas/` | confirmed |
| `jsonschema` dev-dep pinned `0.46` in `crates/core` (S3 reuses this version) | `crates/core/Cargo.toml:86`; `Cargo.lock` `0.46.5` | confirmed |
| Bin name is `hdx` (so `CARGO_BIN_EXE_hdx` resolves) | no `[[bin]]`, default `src/main.rs` | confirmed |
| `clap` 4 default usage-error exit code = 2 (S2's conditional claim) | clap documented default | resolves true; S2 also asserts it in a test |

The plan's "Ground truth" and "Error → exit-code reconciliation" sections are
**accurate**, including the subtle and load-bearing point that a `conformant:false`
report (exit 1) is a distinct `Result` shape from a structural/entry `Err` (exit 2).

---

## Folded STEP-2 issues — all genuinely incorporated (not cosmetic)

1. **Exit-code mapping reconciled with MS6 reality — PRESENT (S2).**
   S2 implements and asserts the full matrix: `describe`+`validate` on `valid/minimal` → 0;
   `validate` on `invalid/missing-root-rollup` → 1 (an `Ok(report)` with `conformant:false`,
   explicitly "the L1 fail is carried in the report, **not** as an error"); `validate`/`describe`
   on `invalid/wrong-format-version` → 2 (the §0 hard cut is a verb `Err`, never softened into a
   report); nonexistent path → 2 (`ManifestUnreadable`); bad args → 2. The exit-1≠exit-2
   distinction is called out as load-bearing and asserted. This matches the verified code
   behavior precisely.

2. **Thin glue ONLY — PRESENT (S1, held in S2/S3).**
   S1 confines `main.rs` to arg-parse (clap) → call the `hdx-core` verb → serialize the returned
   `Description`/`ValidationReport` to JSON on stdout → (S2) map to exit code. It reuses the
   MS5/MS6 serializers (`describe_json`, `report.to_json_string()`) and explicitly does **not**
   re-derive the wire shape or re-implement any §14 rule. Diagnostics go through `tracing` to
   stderr; the JSON on stdout is output. A review-gate "no contract logic in `main.rs`" appears in
   every step's acceptance. Matches architecture §2 / spec §10.

3. **Schema-conformant output — PRESENT (S3).**
   S3 asserts `hdx describe` stdout validates against `schemas/describe.schema.json` and `hdx
   validate` stdout validates against `schemas/validate.schema.json`, **including** the
   `conformant:false` report from `missing-root-rollup`, using the `jsonschema` 0.46 dev-dep at
   the bin-test level. Faithful, non-reshaping surface proven end-to-end through the process
   boundary.

---

## Scope, ordering, green, conventions

- **Scope (clean).** No `regrid`/`clip`/`reduce`. No transform/role/semantic/provenance field, no
  derivable manifest field, no manifest construction/mutation. No new reader, no chunk/pixel
  decode (LOW-3 holds transitively — the CLI only calls the verbs). No new spec MUST-check; the
  CLI exposes MS6's enforcement of the full §14 list. `format_version` stays a hard cut surfaced
  as exit 2. No MS8 (no exhaustive invalid family, no golden outputs) and no MS9 (no PyO3) work.
- **Ordering (buildable as written).** S1 (wire verbs, default exit) → S2 (exit-code mapping +
  assertions) → S3 (schema-conformance + docs). No step depends on a later step. S2's exit-code
  logic has the verbs to map only because S1 wired them; S3 asserts properties of output produced
  by S1 and codes fixed by S2. Correct.
- **Independently green/committable.** S1 lands `clap` + the two subcommands + smoke tests that
  only assert "stdout parses as a JSON object" (no exit-code/schema pin yet) — green with the
  default exit status. S2 changes `main` to return `ExitCode` and updates the same `tests/cli.rs`
  in one coherent commit (no unrelated bundling) — green. S3 adds tests + a README only, runtime
  unchanged — green. Each runs build+test+`clippy --all-targets -- -D warnings`.
- **Conventions honored.** `tracing` to stderr (never `println!` for *diagnostics*); the only
  `println!` is the JSON *output value*, which architecture §2 explicitly permits ("the CLI emits
  JSON via `serde_json` to stdout, which is output, not logging"). `anyhow` + `.context()` in the
  bin (application code), not `thiserror`. `.expect("reason")` reserved for the unrecoverable
  subscriber init (allowed in CLI glue). No `bool`-for-domain-state introduced. No raw primitive
  leaks a domain type (the bin sits *at* the boundary and only parses a path + routes a verb
  result; no domain newtype is bypassed). Per-step `bump-version.sh patch` + stage `Cargo.toml` +
  conventional commit + `git tag`. Commit messages are conventional (`feat(cli):`, `test(cli):`).
- **Coverage.** Every MS7 deliverable and exit criterion from `milestones.md` maps to a step (the
  plan's own coverage table is accurate): subcommands (S1), describe/validate JSON to stdout (S1
  emit + S3 schema-assert), 0/1/2 table implemented (S2), each code asserted by a CLI test (S2),
  `anyhow`/`tracing`-to-stderr (S1+S2), stdout matches schema (S3), no-contract-logic review gate
  (all steps), no new MUST-check (all steps), bump+tag (each step).

---

## Issues (all low — none block approval)

### L-1 (low, convention/clarity) — S3 README location is under-specified
S3 says the README goes "e.g. at the repo root or `src/`-adjacent." A 1-line root `README.md`
already exists, and AGENTS.md mandates crate-level READMEs only for *complex* crates (the bin is
deliberately thin). This is acceptable either way, but the step should name one target so the
implementer does not silently clobber the existing root stub or scatter docs.
**Suggested fix:** pin the README target explicitly (e.g. extend the existing root `README.md`
with a `## CLI` section, or create `src/README.md` for the bin) so review knows where to look.

### L-2 (low, convention/clarity) — make the stdout write unambiguously "output, not diagnostic"
S1 emits the JSON via `println!` of the JSON string. This is within the documented exception
(JSON on stdout is *output*), and the plan is careful to say so — but architecture §2 line 79 also
states "never `println!`" in its headline. To keep the "no `println!`" review gate frictionless,
consider writing the JSON via `std::io::stdout().write_all(...)` (or `writeln!`), reserving
`println!` for nothing. Not required — the parenthetical exception covers `println!` of the output
value — but it removes any reviewer ambiguity.

### L-3 (low, acceptance precision) — S2's clap exit-code leg is phrased conditionally
S2 says "if `clap`'s default is 2 this is satisfied directly." clap 4's `Parser::parse()` does exit
with code 2 on a usage error, and S2 already asserts the observed code in a test, so this resolves.
The conditional phrasing is harmless given the test, but the acceptance would be crisper stated as
a fact ("clap's usage-error exit is 2; a test asserts it") rather than a conditional.

---

## Why APPROVED

Zero high/critical issues. Full coverage of every MS7 deliverable, exit criterion, and spec ref
(§10, architecture §2, §0 hard cut, §14 report-vs-error split, R4). Correct, buildable ordering;
each step independently green and in scope. All repo conventions honored. All three folded STEP-2
issues are genuinely incorporated and match the verified code behavior — especially the
load-bearing exit-1 (`conformant:false` report) vs exit-2 (structural/entry `Err`, including the
§0 hard cut) distinction, which is backed by a committed MS6 test. The only findings are three
low-severity clarity/precision nits that do not affect correctness, scope, or greenness.
