# `hdx` — Python binding (PyO3, maturin)

A thin [PyO3](https://pyo3.rs) extension that exposes the `hdx-core` contract
verbs to Python. The importable module is `hdx`; the crate is `hdx-python`.

## Mirrors `hdx-core` — adds no contract logic

This binding **mirrors** `hdx-core` and adds **zero** contract logic. It is a
pass-through: each Python function calls the matching `hdx-core` `*_json` verb
and parses the already-produced JSON string into a Python `dict`.

| Python call | wraps (`hdx-core`) | returns |
|---|---|---|
| `hdx.describe(path)` | `describe::describe_json` | `dict` with keys `manifest, basins, fields, grids, time_extents, delineations` |
| `hdx.validate(path)` | `validate::validate_json` | `dict` with keys `checks, conformant` |

No §14 rule, no manifest parse, no reader, no discovery lives here — all of that
is in `hdx-core`. Because the binding reuses the verb's JSON string verbatim, the
returned `dict` matches `schemas/describe.schema.json` / `schemas/validate.schema.json`
by construction (architecture R4) — no wire shape is re-derived.

## §0 hard cut preserved end-to-end

A wrong manifest `format_version` is the spec §0 **hard cut**. It surfaces from
`hdx-core` as an error, and the binding maps **exactly that** to the dedicated
exception `hdx.UnknownFormatVersionError` (a subclass of the base `hdx.HdxError`).
It is **never** softened into a `conformant: false` report:

- A wrong `format_version` → **raises** `hdx.UnknownFormatVersionError`.
- A violated §14 MUST that *ran* → a normal `validate(...)` return with
  `conformant: false` (NOT an exception).

```python
import hdx

try:
    report = hdx.validate("/path/to/dataset")  # dict; report["conformant"] is a bool
except hdx.UnknownFormatVersionError:
    ...  # the §0 hard cut — wrong format_version, never a softened report
except hdx.HdxError:
    ...  # any other structural / entry failure
```

## Build & usage (maturin)

The wheel is a single abi3 wheel against the CPython **3.12** stable ABI
(`requires-python >= 3.12`). Develop or build it with maturin from this crate dir:

```bash
# install into the active venv (editable-style; rebuilds on change)
maturin develop

# OR produce a wheel under target/wheels/
maturin build

# then, from Python:
python -c 'import hdx; print(hdx.describe("/path/to/dataset"))'
python -c 'import hdx; print(hdx.validate("/path/to/dataset")["conformant"])'
```

> The `extension-module` PyO3 feature is **optional and non-default** (see
> `Cargo.toml`): plain `cargo build/test/clippy` link with it OFF so the `rlib`
> unit-test target builds on macOS, while `maturin` enables it for the shipped
> wheel only (`[tool.maturin] features`).

## Running the Python tests

### Regenerate-first prerequisite

The integration tests in `tests/` run over the conformance fixture trees
(`conformance/valid/`, `conformance/invalid/`). That fixture **data is
git-ignored** (architecture R2 / §7) and must be regenerated from the
deterministic generator before the tests can read it:

```bash
PYTHON=python3.12 conformance/generator/regenerate.sh
```

### Isolated binding venv

The tests use a **dedicated** binding venv, `crates/python/.venv` — *separate*
from the generator's own `conformance/generator/.venv`. The two interpreters never
collide: the generator manages its venv, and the binding tests get their own.

The one-shot runner does the full sequence (regenerate → create
`crates/python/.venv` → `maturin develop` `hdx` into it → install `pytest` → run
`pytest`) and never mutates the generator venv:

```bash
crates/python/run_python_tests.sh
# or pin the interpreter explicitly:
PYTHON=python3.12 crates/python/run_python_tests.sh
```

The Python test layer is **dev-only tooling** — it is not part of the shipped
wheel.
