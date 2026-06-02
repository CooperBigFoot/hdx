# HDX conformance fixtures

This directory holds the on-disk conformance fixtures for HDX v0.1 — one valid
dataset plus minimal invalid datasets — and the dev-only Python harness that
generates them under [`generator/`](generator/).

There is **no HDX writer in v0.1** (spec §10; architecture §7 **R2**): `validate`
and `describe` are read-only. Yet MS3/MS4 readers and MS6 `validate` need real
on-disk parquet / Zarr / COG / geoparquet bytes to test against. This harness
fills that gap by *emitting bytes a reader will later read* — it is a test fixture
tool, not part of the shipped contract.

> **Status (MS2-S1):** harness only. The pinned generator project, the
> `regenerate.sh` entry point (currently a stub that exits 0), and these three
> load-bearing rules are in place **before any fixture exists**. The valid
> baseline (MS2-S2/S3) and the two derived invalids (MS2-S4) land in later steps.

## Regenerate

```sh
conformance/generator/regenerate.sh
# or, to pin the interpreter explicitly:
PYTHON=python3.12 conformance/generator/regenerate.sh
```

The script idempotently creates a pinned venv (CPython **3.12.x**; see
[`generator/pyproject.toml`](generator/pyproject.toml)), installs the exact-version
closure from [`generator/requirements.lock`](generator/requirements.lock),
smoke-imports every pinned dependency to prove the pins resolve, prints one status
line, and exits 0. At MS2-S1 it emits no fixtures.

---

## The three load-bearing rules

These rules are recorded here **before any fixture exists** so a future agent
treats them as contract, not afterthought. They are also restated in the
generator source ([`generator/hdx_fixtures/__init__.py`](generator/hdx_fixtures/__init__.py)).

### Rule 1 — The generator is DEV-ONLY and is NOT an HDX writer

The generator lives **only** under `conformance/generator/`. It is:

- **never shipped in `hdx-core`**, never imported by, linked from, or depended on
  by any Rust crate or production code;
- **not an HDX writer** — HDX defines no writer in v0.1. The generator does not
  implement or execute any contract logic (that lives exclusively in `hdx-core`,
  per architecture §2). It engineers the on-disk *preconditions* for the spec
  checks so MS3–MS6 can read and enforce them; it enforces nothing itself.

Its own checks are **writer-side self-assertions** (Python), distinct from the
Rust-side enforcement in `validate`. Diagnostics in the generator go through the
standard `logging` machinery (to stderr), never raw `print`; the single
user-facing status line is *output* — mirroring the architecture §2 split between
diagnostics and output.

This is the milestones.md MS2 "generator masquerading as a writer" risk, closed
explicitly.

### Rule 2 — LOW-2: derived, not hand-authored (HARD RULE)

Every invalid fixture (and the larger MS8 invalid family later) **MUST** be
generated **programmatically from the single valid baseline via exactly one
surgical mutation each**. The generator builds the valid baseline once, then
derives each invalid by applying one targeted mutation (e.g. overwrite
`manifest.json`'s `format_version`; delete one root rollup).

> A contributor **MUST NOT** hand-edit a fixture tree. To add or change an invalid
> fixture, add a mutation to the generator and regenerate.

This keeps every fixture exactly one mutation off a known-good baseline, so
"differs in exactly one way" is true by construction and the whole suite is
maintainable as one generator rather than N hand-built trees. A generation-time
self-assertion (added in MS2-S4) confirms each invalid differs from the baseline
in exactly the one intended way.

### Rule 3 — MED-5: Rust-side confirmation hand-off (MS3 / MS4)

The generator's self-assertions are **Python-side**: they assert what the
*writer* intended, which cannot prove what a *Rust reader* recovers from the same
bytes. Two engineered properties are most at risk of a writer/reader mismatch:

1. **Parquet `time` row-group statistics** — pyarrow may or may not emit usable
   min/max statistics for the timestamp logical type under the chosen settings.
   The generator self-asserts the *written file* carries them; **MS3 MUST confirm
   from the Rust side** (`arrow`/`parquet`) that the time extent is sourced from
   those statistics (not a bounded-scan fallback) on the valid fixture.
2. **Zarr v3 consolidated metadata** — `zarr-python`'s v3 consolidated-metadata
   layout must be readable by Rust `zarrs`. The generator self-asserts
   consolidated metadata is present; **MS4 MUST confirm from the Rust side** that
   it reads the store's metadata via the §8 consolidated path (or explicitly
   classify it an R3 byte-deep skip, with a stated reason).

> **The hand-off rule:** if MS3/MS4 find the Rust reader cannot recover a property
> the generator asserted, the fix is to **REGENERATE the fixture** (adjust the
> generator and re-emit) — **never** to add a reader workaround. A mismatch is a
> generator bug, not a reader bug.

---

## Inert / agnostic discipline

No fixture exists yet, so there is nothing to violate at MS2-S1 — but the rule
holds for every later step: `manifest.json` is **exactly** the six floor fields
(spec §11); no content hash, no data-version, no field catalog, no
transform/role/semantic/provenance key. Field names are opaque producer strings;
the `{source}_{variable}` and companion-mask `{field}_was_filled` patterns appear
(in later steps) **only to prove later milestones give them no special handling**.
`format_version` is a **hard cut** (`"0.1"` here).

## Layout (target, populated by later steps)

```
conformance/
  README.md                          # this file
  generator/                         # dev-only Python harness (NOT shipped in hdx-core)
    pyproject.toml                    # pinned deps + interpreter (CPython 3.12.x)
    requirements.lock                 # exact-version lock installed by regenerate.sh
    regenerate.sh                     # entry point (MS2-S1: stub, exits 0)
    hdx_fixtures/                     # package: logging-configured generator modules
  valid/minimal/                      # one valid four-quadrant dataset   (MS2-S2/S3)
  invalid/wrong-format-version/       # pins M2 — one surgical mutation   (MS2-S4)
  invalid/missing-root-rollup/        # pins L1 — one surgical mutation   (MS2-S4)
```
