# MS9 step-plan critique — adversarial review

**Milestone:** MS9 — PyO3 binding (`crates/python`, maturin). The LAST v0.1 milestone.
**Plan reviewed:** `planning/MS9/steps.md` (S1–S4).
**Verdict:** **NOT APPROVED.** Two HIGH not-green defects in the toolchain wiring
(S1/S2) would leave the workspace red on this host on the very first step, in direct
contradiction of the milestone's hard "whole workspace green under
`cargo build` / `cargo test` / `cargo clippy --all-targets -- -D warnings`" gate.

The plan's *scope*, *ordering*, *coverage*, and *mirror discipline* are otherwise
excellent — every folded STEP-2 issue is genuinely incorporated. The failure is
narrow but blocking: the PyO3 crate is wired in a way that does not build green on
the Darwin host the milestone-build skill actually runs on.

---

## What the plan gets right (verified against the repo)

- **Starting state is accurate.** `crates/python` does not exist; the workspace is
  `members = ["crates/*"]`; `hdx-core` exposes exactly the verbs the plan names —
  `describe_json`/`validate_json`/`validate` and `ValidationReport::conformant()`
  (`crates/core/src/describe.rs:355`, `crates/core/src/validate.rs:1308`,
  `crates/core/src/validate.rs:383`). `python3.12` (3.12.8) and `maturin` (1.13.1)
  are on the host.
- **Mirror, never reimplement (§10) — folded.** S2 calls `describe_json`/
  `validate_json` and transports the JSON string to a `dict`; it explicitly forbids
  re-parsing the manifest, re-walking the tree, or re-deriving the wire shape.
  Output matches `schemas/describe.schema.json` / `schemas/validate.schema.json`
  by construction. The §13 inert discipline holds (binding adds no domain type).
- **§0 hard cut preserved end-to-end — folded.** The error surface the plan relies
  on is real: `DescribeError::Manifest(#[source] CoreError)` /
  `ValidateError::Manifest(#[source] CoreError)` wrap `CoreError::UnknownFormatVersion`
  unchanged (`crates/core/src/error.rs:331`, `:412`). S2 maps
  `Manifest(CoreError::UnknownFormatVersion { found })` to a distinct Python
  exception, never softened; S3 asserts both verbs *raise* on the
  `invalid/wrong-format-version` fixture (which exists on disk).
- **Fixtures-gitignored / regenerate-first — folded.** `git ls-files
  conformance/valid conformance/invalid` returns **0** tracked files; goldens live
  under `conformance/goldens/`. S3's runner runs `PYTHON=python3.12
  conformance/generator/regenerate.sh` first; S4 documents the prerequisite and
  re-verifies the goldens-intact / no-fixture-data invariant. The fixtures S3 names
  (`valid/minimal`, `invalid/missing-root-rollup`, `invalid/wrong-format-version`)
  all regenerate, and `validate(invalid/missing-root-rollup)` genuinely returns a
  `conformant:false` *report* (not an `Err`) — confirmed by the L1 test at
  `crates/core/src/validate.rs:1999`. The describe top-level keys S3 asserts
  (`time_extents` plural, etc.) match `schemas/describe.schema.json` and the
  committed `conformance/goldens/valid-minimal.describe.json`.
- **Scope — clean.** Only `validate` + `describe` are exposed. No
  `regrid`/`clip`/`reduce`, no new `hdx-core` domain/manifest field, no
  inert-violating field, no `hdx-core` source change. This is genuinely minimal and
  closes v0.1.
- **Ordering — sound in principle.** S1 (skeleton/link) → S2 (verbs+mapping) →
  S3 (Python E2E) → S4 (docs + close-out) is correctly dependency-sequential; the
  §0 behaviour is *implemented* in S2 before S3 *asserts* it.

---

## Issues

### HIGH-1 — `extension-module` enabled unconditionally breaks `cargo test` / `cargo clippy --all-targets` on the Darwin host (not-green)

**Where:** S1 changes, `crates/python/Cargo.toml` line 140:
`pyo3 = { version = "<pin>", features = ["abi3-py312", "extension-module"] }`, with
`[lib] crate-type = ["cdylib"]` only.

**Problem.** This host is **Darwin arm64** (`uname` = `Darwin arm64`; the
milestone-build skill executes `cargo build/test/clippy` here). PyO3's
`extension-module` feature tells the linker to **leave the Python symbols
undefined**, to be resolved by the host interpreter at `import` time. That is correct
for the shipped `cdylib`, but it is the canonical PyO3 gotcha that, on **macOS and
Windows**, *any non-cdylib target* — the unit-test executable, an integration-test
binary, a doctest, a bench — **fails to link** with undefined `_Py*` symbols, because
those targets are linked eagerly with no interpreter to satisfy the symbols. The PyO3
book documents this explicitly.

The milestone's hard gate (steps.md "Scope guard", lines 59–62, repeated in every
step's Acceptance) is: *after every step* `cargo build`, `cargo test`, **and**
`cargo clippy --all-targets -- -D warnings` pass for the **whole workspace including
`crates/python`**. With `extension-module` on unconditionally:
- **S1** already plans a Rust unit test in `crates/python` (lines 170–173) → the test
  target won't link on macOS → `cargo test` **red on the very first MS9 commit**.
- **S2** plans Rust unit tests that call `Python::with_gil` (lines 247–252) → same
  link failure, worse (they also need a real interpreter at runtime).
- `cargo clippy --all-targets` compiles test/bench targets too, so the
  `-D warnings` gate is also at risk on the same targets.

The plan **never mentions gating `extension-module`**. The standard, required fix is
an *optional, off-by-default* feature, e.g.
`[features] extension-module = ["pyo3/extension-module"]` (NOT a default), enabled
only for the maturin wheel build (maturin enables it automatically) and **left off**
for `cargo test`/`cargo clippy`; or equivalently, gate the GIL-using Rust tests
behind a feature and run them only via the Python harness. None of this is in the
plan. As written, S1 cannot leave the tree green on this host.

**Severity:** HIGH (a step that leaves the tree red, on the first commit, against an
explicit milestone exit criterion).

**Suggested fix.** In S1, declare `extension-module` as an **optional, non-default**
feature that re-exports `pyo3/extension-module`; do not put it in the unconditional
`features` list. Document that `cargo build/test/clippy` run **without** it and
`maturin build`/`develop` enable it (maturin does so by default). Add an explicit
S1 acceptance line: `cargo test -p hdx-python` and `cargo clippy --all-targets -p
hdx-python -- -D warnings` are green on macOS with the default (no
`extension-module`) feature set. State the macOS link gotcha in the architecture.md
amendment as the load-bearing non-obvious setup the task asks to record.

---

### HIGH-2 — `crate-type = ["cdylib"]` only → the planned Rust unit tests have no target to build (not-green)

**Where:** S1 line 139 (`[lib] crate-type = ["cdylib"]`), against S1 line 170 ("A
Rust unit test in `crates/python` … asserts the trivial function …") and S2 lines
247–252 ("Rust unit tests in `crates/python` for the error-mapping helpers …").

**Problem.** A **cdylib-only** crate produces no `lib`/`rlib` rustc target, so
`#[cfg(test)]` unit tests inside `src/lib.rs` are **not compiled or run** by
`cargo test`, and integration tests under `tests/` cannot link the crate as a
dependency (they need an `rlib`). Both S1 and S2 explicitly plan Rust-side
`cargo test` coverage (the trivial-probe assertion in S1; the error-mapping helper
tests in S2 that are the *only* Rust-level proof of the §0 hard-cut mapping before
S3). With `crate-type = ["cdylib"]` alone, those tests **do not exist as a buildable
target** — the plan's S2 acceptance ("verified by the Rust mapping unit test", line
266) is unrealizable as specified.

**Severity:** HIGH (the plan's own acceptance evidence for the §0 mapping cannot be
produced; bundled with HIGH-1 it means `crates/python` has no green Rust test path).

**Suggested fix.** Set `crate-type = ["cdylib", "rlib"]` in S1 so the lib is both the
Python extension and a normal Rust library that unit/integration tests can build
against. Pair this with HIGH-1's optional-feature gating so the `rlib` test target
links without `extension-module`. (PyO3's own examples use exactly
`["cdylib", "rlib"]` for testable extension crates.)

---

### MEDIUM-1 — S3 does not specify *which* venv `maturin develop` targets; a generator venv already exists and collides conceptually

**Where:** S3, `crates/python/tests/run_python_tests.sh` (lines 314–318): "(1)
regenerate … (2) `maturin develop` builds & installs `hdx` into the python3.12 venv".

**Problem.** A venv already exists at `conformance/generator/.venv` (created by
`regenerate.sh`, pinned to the fixture-generator dependency closure: pyarrow, xarray,
zarr, rioxarray, geopandas, …). The plan says "the python3.12 venv" as if there is
one canonical venv, but it never says **create a dedicated binding venv**. If the
runner reuses the generator venv, `maturin develop` mutates a venv whose purpose is
fixture generation (and whose lock the regenerate stamp checks via
`.venv/.lock.sha256`), risking a stale-lock rebuild or import-environment confusion;
if it silently picks an ambient `python3.12`, the regenerate step (which insists on
its own venv) and the test step run under different interpreters. Either way the
"regenerate-first then import `hdx`" workflow is under-specified for a reproducible
green run.

**Severity:** MEDIUM (reproducibility/ordering ambiguity in the milestone's
behavioural-proof step; not a guaranteed failure, but a real flake/contamination
risk the plan should close).

**Suggested fix.** S3 should explicitly create a **separate** binding test venv
(e.g. `crates/python/.venv`, already covered by the `.gitignore` patterns S1 adds),
install `hdx` into *that* venv via `maturin develop`, and run `pytest` from it —
keeping the generator's `conformance/generator/.venv` untouched. State the exact
venv path so the runner is deterministic, and confirm the regenerate step (its own
venv) and the pytest step (the binding venv) are isolated.

---

### LOW-1 — S1 maturin acceptance is a manual/"succeeds" gate, not a concrete artifact assertion

**Where:** S1 Acceptance (lines 182–183): "`maturin build -m crates/python/Cargo.toml`
succeeds against python3.12 (abi3 wheel produced)."

**Problem.** For the LAST milestone, `maturin build` succeeding is load-bearing, but
"succeeds" is softer than the rest of the plan's acceptance (which names concrete
commands and outputs). Given HIGH-1, "the cdylib links" and "maturin builds" are
exactly the claims most likely to be wrong on macOS, so the gate should assert the
**produced wheel filename/abi3 tag** (e.g. a `*-abi3-*.whl` under
`target/wheels/`), not merely a zero exit.

**Severity:** LOW (acceptance quality).

**Suggested fix.** Make S1 acceptance assert the wheel artifact exists with the abi3
platform tag, and that `python3.12 -c "import hdx; hdx.__core_version()"` works after
`maturin develop` (the import/link proof S1 claims to provide but defers wholly to
S3).

---

## Coverage check (every MS9 deliverable + exit criterion + spec ref)

| MS9 deliverable / exit criterion | Covered? | Note |
|---|---|---|
| `crates/python` PyO3 crate in workspace + maturin `pyproject.toml` | S1 | wiring correct; **but** feature/crate-type defects (HIGH-1/2) |
| abi3 + extension-module cdylib; `maturin build`/`develop` succeeds | S1 / S3 | extension-module must be **conditional** (HIGH-1) |
| `hdx.describe(path)`/`hdx.validate(path)` → `dict` | S2 | correct, thin |
| each a thin wrapper (mirror, not reimplement) | S2 (impl), S4 (doc) | correct |
| error mapping incl. §0 hard cut → distinct exception | S2 | correct; Rust proof blocked by HIGH-2 |
| §0 hard cut preserved end-to-end | S2 (map), S3 (assert) | correct |
| Python tests: describe/validate over valid → expected structures | S3 | correct |
| Python tests: validate over invalid → `conformant:false` | S3 | correct (`missing-root-rollup` reports, not errs) |
| Python tests: wrong-version → hard-cut exception | S3 | correct |
| regenerate-first prerequisite, no committed fixture data | S3/S4 | correct; **venv under-specified (MED-1)** |
| `crates/python/README.md` (build+usage+mirror+regenerate-first) | S4 | correct |
| workspace green incl. `crates/python` every step | S1–S4 | **NOT MET on macOS as written (HIGH-1/2)** |
| goldens intact after clean regenerate; no golden under gitignored trees | S4 | correct invariant |
| record non-obvious build setup as architecture.md amendment | S1 | should also record the macOS link gotcha (HIGH-1) |
| commit via bump+tag each step | S1–S4 | conventional messages present |
| spec MUST-checks: none new; M1/M2 §0 hard cut through binding | S2/S3 | correct |

Coverage of *content* is complete; the gap is the green-gate, not a missing
deliverable.

---

## Conventions

- No `println!` for diagnostics, no new `bool` domain state, no raw primitive past
  a boundary (paths parse into `hdx-core` at the verb call), no `use super::*`,
  module `//!` doc planned (S2) — all honored.
- "No `.unwrap()`/`.expect()` in library code": the wrappers return `PyResult` and
  every error arm raises (S2). Acceptable. Note the GIL test code (S2) may use
  `expect` in `#[cfg(test)]` — fine, but those tests are themselves blocked by
  HIGH-1/HIGH-2 and must be re-homed/feature-gated.
- `thiserror` doc-comments: N/A — the binding defines Python exceptions via
  `pyo3::create_exception!`, not `thiserror`; it reuses `hdx-core`'s already-doc'd
  error enums. Correct (mirror, no new contract error type).

---

## Required changes to approve

1. **HIGH-1:** Make `extension-module` an optional, **non-default** feature
   (`[features] extension-module = ["pyo3/extension-module"]`); run
   `cargo build/test/clippy` without it; let maturin enable it for the wheel. Add an
   explicit macOS-green acceptance line and record the link gotcha in the
   architecture amendment.
2. **HIGH-2:** Set `crate-type = ["cdylib", "rlib"]` so the planned Rust unit/
   integration tests have a target. (Combine with HIGH-1 so the test target links.)
3. **MEDIUM-1:** Pin S3's runner to a **dedicated** binding venv (e.g.
   `crates/python/.venv`), isolated from `conformance/generator/.venv`; state the
   path so the regenerate-first + import workflow is deterministic.
4. **LOW-1:** Strengthen S1's maturin acceptance to assert the abi3 wheel artifact +
   a real `import hdx` smoke.

With HIGH-1 and HIGH-2 fixed (and MED-1/LOW-1 folded), the plan would be
approvable: scope, ordering, coverage, mirror discipline, and the §0 end-to-end
guarantee are all correct.
