# MS9 STEP-plan critique (iteration 2) — adversarial review

**Milestone:** MS9 — PyO3 binding (`crates/python`, maturin). The LAST v0.1 milestone.
**Plan reviewed:** `planning/MS9/steps.md` (S1→S2→S3, iteration 2).
**Verdict: APPROVED** (severity: low). Zero high/critical issues. Full deliverable
+ exit-criterion + spec-ref coverage. Correct S1→S2→S3 ordering, each step
independently green and in scope. Conventions honored. All four STEP-2 issues
genuinely folded (not cosmetic). The low-severity notes below are polish, not
blockers.

---

## Verification performed (repo ground truth, not just the plan's self-claims)

Every "Verified starting state" claim in `steps.md` was independently checked
against the repo:

- **Verb signatures match.** `hdx_core::describe::describe_json(path) -> Result<String, DescribeError>`
  (`crates/core/src/describe.rs:355`) and
  `hdx_core::validate::validate_json(path) -> Result<String, ValidateError>`
  (`crates/core/src/validate.rs:1308`) exist with exactly the claimed shapes; the
  typed `describe`/`validate` variants exist too.
- **Boundary error enums match.** `DescribeError`/`ValidateError`
  (`crates/core/src/error.rs:306,382`) each carry `ManifestUnreadable {path,detail}`,
  `Manifest(CoreError)`, `Discovery(CoreError)`, `Serialize {detail}`. The §0 hard
  cut surfaces as `Manifest(CoreError::UnknownFormatVersion { found })` — confirmed
  by doc text "a caller can match `Manifest(CoreError::UnknownFormatVersion { .. })`".
  It is an `Err`, never softened into a report.
- **`missing-root-rollup` really returns `conformant:false` (Ok), not an Err.** The
  fixture removes `outlines.geoparquet` (it keeps `scalar_static.parquet`);
  `discovery.rs:30` documents "discovery still succeeds (L1 enforcement is MS6)", and
  L1 is a recorded `ran:fail` outcome (`validate.rs:741-757`). So S3's invalid-fixture
  assertion (`conformant is False`, not a raised exception) is sound.
- **`wrong-format-version` manifest has `format_version: "0.2"`** (confirmed) → an
  `Err(Manifest(UnknownFormatVersion))` at the entry gate, before discovery → S3's
  `pytest.raises(<hard-cut>)` assertion is sound.
- **Fixtures-gitignored policy is in effect.** `git ls-files conformance/valid/*
  conformance/invalid/*` = **0** files; `.gitignore` ignores `conformance/valid/**`
  + `conformance/invalid/**`; goldens are tracked under `conformance/goldens/`
  (11 files). No golden lives under the gitignored trees.
- **`regenerate.sh` rebuilds all three needed trees** (valid/minimal,
  invalid/wrong-format-version, invalid/missing-root-rollup), uses its own venv at
  `conformance/generator/.venv`, honors `PYTHON=python3.12`. MED-1 isolation is
  well-founded: `maturin develop` into the generator venv would corrupt its pinned
  closure.
- **Toolchain present:** `/usr/local/bin/python3.12` (3.12.8),
  `/opt/homebrew/bin/maturin` (1.13.1), host is Darwin **arm64** — the macOS PyO3
  link gotcha is real and S1 designs around it from the first commit.
- **Schema keys match the plan's assertions exactly:** describe →
  `manifest, basins, fields, grids, time_extents, delineations`; validate →
  `checks, conformant`. S3 test #1's key assertions are correct.
- **Workspace** `members = ["crates/*"]` auto-includes a new `crates/python`;
  `cargo build` is currently green.

---

## STEP-2 fold verification (each genuinely incorporated)

1. **Mirror, never reimplement (§10).** FOLDED. S2's "Mirror discipline" block
   binds the binding to call **only** `describe_json`/`validate_json` and transport
   the produced JSON unchanged; the acceptance gate ("each calls only `hdx-core`'s
   `*_json` verb … verifiable by review of `lib.rs`") and the per-step scope-guard
   statement enforce no §14/contract/wire-shape logic in the binding. Schema match
   is "by construction" because the exact `*_json` strings are reused. The §0 hard
   cut maps to a dedicated `UnknownFormatVersion`-style exception (S2) and is proven
   end-to-end to Python (S3 `pytest.raises`), never softened to `conformant:false`.

2. **Fixtures-gitignored / regenerate-first.** FOLDED. S3 runs
   `PYTHON=python3.12 conformance/generator/regenerate.sh` first, commits no fixture
   bytes, and states the prerequisite in `crates/python/README.md`. The isolation
   (MED-1) into a dedicated `crates/python/.venv` (gitignored by S1) leaving the
   generator venv untouched is explicit, and "clean regenerate leaves goldens intact"
   is asserted.

3. **Workspace + build gate.** FOLDED. S1 adds the abi3 PyO3 extension; the whole
   workspace stays green under `cargo build`/`test`/`clippy --all-targets -- -D warnings`
   at **every** step, and `maturin develop`/`build` succeeds with a real
   `python3.12 -c "import hdx; hdx.__core_version()"` import proof. The README
   documents maturin build + usage + mirrors-no-contract + regenerate-first.

4. **LAST milestone, minimal + in scope.** FOLDED. Only `validate` + `describe`
   exposed; no `regrid`/`clip`/`reduce`, no new `hdx-core` field, no inert violation;
   `hdx-core` is explicitly frozen. abi3 + python3.12 used; the macOS link setup is
   recorded as an `architecture.md` Amendments entry by the implementer at S1.

---

## Issues filed (all LOW — none blocking)

### LOW — `version mirrored to the workspace bump convention` is imprecise (S1)

`steps.md` line 165 says the new `crates/python/Cargo.toml` version is "mirrored to
the workspace bump convention." But `scripts/bump-version.sh` edits **only the root
`Cargo.toml`** version field (the `hdx` bin). `hdx-core` has sat at `0.1.0` across
54 root bumps and is never touched by the script; a new `crates/python` follows the
same pattern — it carries its own static version and is **not** bumped per commit.
The commit-step text ("`./scripts/bump-version.sh patch`, stage `Cargo.toml`") is
correct (it bumps + stages the root manifest), so this is a wording imprecision, not
a process error. *Fix:* state that `crates/python/Cargo.toml` carries its own static
version (e.g. `0.1.0`, like `hdx-core`) and that only the root `Cargo.toml` is
bumped/staged by `bump-version.sh`.

### LOW — `dict`-vs-string deliverable left as an implementer escape hatch (S2)

S2 names a `dict` as "the default deliverable" but permits "if a `dict` is
impractical, return the JSON string and document that callers `json.loads` it." The
MS9 milestone deliverable is "returning the same structures (dicts) … as Python
objects (or JSON strings parsed to dict)", so the fallback is within milestone
scope. However, S3's tests assert on **dict** subscripting
(`validate(valid)["conformant"]`, key-set checks) — if the implementer takes the
string escape hatch, S3's tests as written would need `json.loads` first.
*Fix (non-blocking):* make S3's tests robust to either by `json.loads`-ing a string
result, OR commit to `dict` as the firm contract and drop the escape hatch. Either
keeps the milestone green; the ambiguity is the only risk.

### LOW — describe JSON key is `time_extents` (plural); confirm not `time_extent` (S3)

The describe top-level time key is `time_extents` (plural; confirmed in
`schemas/describe.schema.json` required-set and the `Description` DTO). `steps.md`
uses `time_extents` in S3's test list (line 380) and at line 25 — consistent and
correct. Note the architecture sketch at `architecture.md:166` shows a Rust field
named `time_extent` (singular); S3 asserts the **JSON wire key**, which is the
plural `time_extents`, so the plan is right. No fix required; recorded to confirm the
naming was checked against the real schema and no drift exists.

---

## Scope audit — clean

- No step adds `regrid`/`clip`/`reduce`, a new `hdx-core` domain/manifest field, or
  any inert-violating field (transform/role/semantic/provenance). `hdx-core` is
  frozen; MS9 adds only `crates/python`, its tests, its README, and one
  `architecture.md` amendment.
- No step does a later milestone's work (MS9 is last).
- The binding never reaches beyond mirroring the two existing verbs.

## Green/committable audit — clean

- **S1** ships a buildable, importable do-nothing extension; the `["cdylib","rlib"]`
  + optional non-default `extension-module` design keeps `cargo test`/`clippy
  --all-targets` linkable on macOS (HIGH-1/HIGH-2 correctly folded). Green.
- **S2** adds the two verbs + error mapping with Rust `#[cfg(test)]` unit tests over
  the rlib target (buildable only because S1 set rlib + gated the feature — ordering
  is correct). Green.
- **S3** adds Python tests + README + runner; no Rust regression. Green.
- Each step is independently committable and leaves build+test+clippy green for the
  whole workspace.

## Ordering audit — clean

S1→S2→S3 is buildable as written: S2's verbs need S1's linkable rlib crate; S3's
Python tests import the functions S2 adds. No step depends on a later step.

## Conventions audit — clean

- No `println!` (S1 explicitly forbids it; `tracing` only if needed).
- No `unwrap`/`expect`/panic baked into library code; errors map to typed Python
  exceptions via `create_exception!`.
- §0 hard cut preserved as a dedicated exception (dedicated type, not a
  bool/softened report).
- No raw-primitive-past-boundary concern: the binding is a boundary that takes a
  path `&str` (the system edge) and immediately delegates to `hdx-core`'s typed
  verbs.
- No manifest extra/missing-field handling re-implemented (delegated to `hdx-core`).
- `create_exception!`-defined exceptions are documented; no `use super::*`.

## Acceptance-quality audit — clean

Acceptance criteria are concrete: explicit `cargo build`/`cargo test`/`cargo clippy
--all-targets -- -D warnings` (whole workspace + `-p hdx-python`), a concrete
`*-abi3-*.whl` artifact + real `python3.12 -c "import hdx; ..."` exit-0 proof,
named spec checks (§0, §10, §14, R4), and conventional commit messages
(`feat(python): …`, `test(python): …`). The coverage table maps every MS9
deliverable + exit criterion to a step with no gaps.
