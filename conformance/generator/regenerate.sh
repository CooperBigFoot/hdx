#!/usr/bin/env bash
#
# regenerate.sh — HDX conformance fixture generator entry point (dev-only).
#
# MS2-S2: idempotently creates the pinned venv, installs the exact-version
# dependency closure, smoke-imports every pinned dep (proving the pins resolve),
# then emits the SCALAR half of the one valid baseline into
# conformance/valid/minimal/ (manifest.json, scalar_static.parquet, per-basin
# scalar_dynamic.parquet, outlines.geoparquet) and runs the load-bearing scalar
# self-assertions, ABORTING on any failure (non-zero exit). The gridded half
# (S3) and the two derived invalids (S4) are wired into this script in later
# MS2 steps.
#
# This generator is DEV-ONLY and is NOT an HDX writer: it lives only under
# conformance/, is never shipped in or imported by hdx-core, and only emits bytes
# a reader will later read (spec §10 / architecture §7 R2). See conformance/README.md.
#
# Reproducibility: the venv is built from a CPython 3.12 interpreter (override with
# PYTHON=/path/to/python3.12), NOT the ambient python3 — some hosts ship 3.14, for
# which the pinned wheels do not yet publish binaries (see pyproject.toml).
#
# Usage:
#   conformance/generator/regenerate.sh           # set up venv + smoke import
#   PYTHON=python3.12 conformance/generator/regenerate.sh
#   HDX_FIXTURES_LOG_LEVEL=DEBUG conformance/generator/regenerate.sh

set -euo pipefail

# --- locate self (works regardless of caller cwd) ---------------------------
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# Repo root is two levels up: conformance/generator/ -> conformance/ -> <repo>.
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
VALID_MINIMAL="${REPO_ROOT}/conformance/valid/minimal"

VENV_DIR="${SCRIPT_DIR}/.venv"
LOCK_FILE="${SCRIPT_DIR}/requirements.lock"
STAMP_FILE="${VENV_DIR}/.lock.sha256"

# --- choose a compatible interpreter ----------------------------------------
# Prefer an explicit PYTHON override, then python3.12, then fall back to python3.
PYTHON_BIN="${PYTHON:-}"
if [[ -z "${PYTHON_BIN}" ]]; then
    if command -v python3.12 >/dev/null 2>&1; then
        PYTHON_BIN="python3.12"
    else
        PYTHON_BIN="python3"
    fi
fi

if ! command -v "${PYTHON_BIN}" >/dev/null 2>&1; then
    echo "regenerate.sh: interpreter '${PYTHON_BIN}' not found (set PYTHON=...)" >&2
    exit 1
fi

# Guard the declared Python range (>=3.12,<3.13): warn loudly if mismatched, since
# the pinned wheels may not resolve on other versions.
PY_OK="$("${PYTHON_BIN}" -c 'import sys; print(1 if (3,12) <= sys.version_info[:2] < (3,13) else 0)')"
if [[ "${PY_OK}" != "1" ]]; then
    PY_VER="$("${PYTHON_BIN}" -c 'import sys; print("%d.%d.%d" % sys.version_info[:3])')"
    echo "regenerate.sh: WARNING: ${PYTHON_BIN} is ${PY_VER}; harness targets 3.12.x." >&2
    echo "regenerate.sh: set PYTHON=python3.12 if the pinned wheels fail to resolve." >&2
fi

# --- idempotent venv setup --------------------------------------------------
if [[ ! -x "${VENV_DIR}/bin/python" ]]; then
    echo "regenerate.sh: creating venv at ${VENV_DIR} (interpreter: ${PYTHON_BIN})" >&2
    "${PYTHON_BIN}" -m venv "${VENV_DIR}"
fi

VENV_PY="${VENV_DIR}/bin/python"

# --- install pinned deps only when the lock changed (idempotent) ------------
LOCK_HASH="$(shasum -a 256 "${LOCK_FILE}" | awk '{print $1}')"
NEED_INSTALL=1
if [[ -f "${STAMP_FILE}" ]] && [[ "$(cat "${STAMP_FILE}")" == "${LOCK_HASH}" ]]; then
    NEED_INSTALL=0
fi

if [[ "${NEED_INSTALL}" == "1" ]]; then
    echo "regenerate.sh: installing pinned deps from $(basename "${LOCK_FILE}")" >&2
    "${VENV_PY}" -m pip install --quiet --upgrade pip
    "${VENV_PY}" -m pip install --quiet --require-virtualenv -r "${LOCK_FILE}"
    echo "${LOCK_HASH}" > "${STAMP_FILE}"
else
    echo "regenerate.sh: pinned deps already installed (lock unchanged)" >&2
fi

# --- smoke import (proves the pins resolve on this interpreter) -------------
# Run from the generator dir so the `hdx_fixtures` package is importable.
cd "${SCRIPT_DIR}"
"${VENV_PY}" -m hdx_fixtures

# --- emit the valid baseline scalar half + run scalar self-assertions -------
# build.py writes manifest.json + scalar_static.parquet + per-basin
# scalar_dynamic.parquet + outlines.geoparquet into conformance/valid/minimal/,
# then runs run_scalar_assertions(); any AssertionFailed aborts with non-zero.
echo "regenerate.sh: emitting valid baseline scalar half -> ${VALID_MINIMAL}" >&2
exec "${VENV_PY}" -m hdx_fixtures.build --dataset-root "${VALID_MINIMAL}"
