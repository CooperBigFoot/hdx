#!/usr/bin/env bash
#
# regenerate.sh — HDX conformance fixture generator entry point (dev-only).
#
# MS2-S1 STUB: this step stands up the harness only. It idempotently creates the
# pinned venv, installs the exact-version dependency closure, smoke-imports every
# pinned dep on the declared interpreter (proving the pins resolve), prints one
# status line, and exits 0. No fixtures are emitted yet — later MS2 steps wire the
# valid baseline (S2/S3) and the two derived invalids (S4) into this script.
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

# --- smoke import + status line (S1: harness only) --------------------------
# Run the package entry point from the generator dir so `hdx_fixtures` is importable.
cd "${SCRIPT_DIR}"
exec "${VENV_PY}" -m hdx_fixtures
