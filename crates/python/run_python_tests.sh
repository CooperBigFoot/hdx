#!/usr/bin/env bash
#
# run_python_tests.sh — deterministic end-to-end runner for the `hdx` PyO3 binding
# tests (MS9-S3, dev-only tooling — NOT shipped).
#
# The Python tests in crates/python/tests/ exercise the binding over the
# conformance fixture trees, which are git-ignored (architecture R2 / §7+§8): the
# fixture DATA is never committed and must be REGENERATED from the deterministic
# generator before any test reads it. This runner makes the whole run reproducible
# from a clean checkout by doing the steps in a fixed order.
#
# Two ISOLATED venvs, never colliding (MED-1):
#   * conformance/generator/.venv  — owned by regenerate.sh; this runner NEVER
#     creates, mutates, or installs into it. regenerate.sh manages it itself.
#   * crates/python/.venv          — the DEDICATED binding test venv this runner
#     creates/refreshes, `maturin develop`s `hdx` into, and runs `pytest` from.
#     It is git-ignored (see root .gitignore).
#
# Order (deterministic):
#   1. PYTHON=python3.12 conformance/generator/regenerate.sh   (regenerate-first)
#   2. create/refresh crates/python/.venv with python3.12
#   3. maturin develop --release-free into crates/python/.venv (builds + installs `hdx`)
#   4. pip install pytest into crates/python/.venv
#   5. pytest crates/python/tests from crates/python/.venv
#
# Usage:
#   crates/python/run_python_tests.sh
#   PYTHON=python3.12 crates/python/run_python_tests.sh   # pin the interpreter

set -euo pipefail

# --- locate self (works regardless of caller cwd, like regenerate.sh) --------
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# Repo root is two levels up: crates/python/ -> crates/ -> <repo>.
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

# The two isolated venv paths, stated explicitly for a deterministic run.
GENERATOR_VENV="${REPO_ROOT}/conformance/generator/.venv"   # owned by regenerate.sh; untouched here
BINDING_VENV="${SCRIPT_DIR}/.venv"                            # dedicated binding test venv

REGENERATE="${REPO_ROOT}/conformance/generator/regenerate.sh"
TESTS_DIR="${SCRIPT_DIR}/tests"

# --- choose a CPython 3.12 interpreter for the BINDING venv ------------------
# Prefer an explicit PYTHON override, then python3.12. The binding wheel is an
# abi3-py312 wheel (Cargo.toml), so the test interpreter must be 3.12+.
PYTHON_BIN="${PYTHON:-}"
if [[ -z "${PYTHON_BIN}" ]]; then
    if command -v python3.12 >/dev/null 2>&1; then
        PYTHON_BIN="python3.12"
    else
        PYTHON_BIN="python3"
    fi
fi

if ! command -v "${PYTHON_BIN}" >/dev/null 2>&1; then
    echo "run_python_tests.sh: interpreter '${PYTHON_BIN}' not found (set PYTHON=...)" >&2
    exit 1
fi

if ! command -v maturin >/dev/null 2>&1; then
    echo "run_python_tests.sh: 'maturin' not found on PATH (install maturin >=1,<2)" >&2
    exit 1
fi

# --- 1. regenerate-first ------------------------------------------------------
# The fixtures are git-ignored, so they MUST be (re)generated before any test
# reads them. regenerate.sh manages conformance/generator/.venv ITSELF; this
# runner never touches that venv. We forward the chosen interpreter via PYTHON.
echo "run_python_tests.sh: [1/5] regenerating fixtures via ${REGENERATE} (generator venv: ${GENERATOR_VENV})" >&2
PYTHON="${PYTHON_BIN}" "${REGENERATE}"

# --- 2. create/refresh the DEDICATED binding venv -----------------------------
echo "run_python_tests.sh: [2/5] (re)creating binding venv at ${BINDING_VENV} (interpreter: ${PYTHON_BIN})" >&2
rm -rf "${BINDING_VENV}"
"${PYTHON_BIN}" -m venv "${BINDING_VENV}"
BINDING_PY="${BINDING_VENV}/bin/python"
"${BINDING_PY}" -m pip install --quiet --upgrade pip

# --- 3. maturin develop `hdx` into the binding venv ---------------------------
# `maturin develop` builds the abi3/extension-module wheel (the optional feature
# is enabled via [tool.maturin] features in pyproject.toml) and installs it into
# THE CURRENT virtualenv — which it detects via VIRTUAL_ENV. We point it at the
# DEDICATED binding venv (never the generator venv) by exporting VIRTUAL_ENV and
# prepending its bin/ to PATH for this one command, and run it from the crate dir
# so it finds pyproject.toml.
echo "run_python_tests.sh: [3/5] maturin develop hdx into ${BINDING_VENV}" >&2
(
    cd "${SCRIPT_DIR}"
    export VIRTUAL_ENV="${BINDING_VENV}"
    export PATH="${BINDING_VENV}/bin:${PATH}"
    maturin develop
)

# --- 4. install pytest into the binding venv ----------------------------------
echo "run_python_tests.sh: [4/5] installing pytest into ${BINDING_VENV}" >&2
"${BINDING_PY}" -m pip install --quiet pytest

# --- 5. run the binding tests from the binding venv ---------------------------
echo "run_python_tests.sh: [5/5] running pytest ${TESTS_DIR} from ${BINDING_VENV}" >&2
exec "${BINDING_PY}" -m pytest "${TESTS_DIR}" -v
