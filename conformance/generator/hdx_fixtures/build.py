"""Build the valid baseline + derive the two invalids, then self-assert (MS2-S4).

This is the generation entry point ``regenerate.sh`` calls. It emits the one
valid dataset into ``conformance/valid/minimal/`` in two halves, then derives the
two minimal invalids from that baseline, running the load-bearing self-assertions
after each step and aborting on any failure:

* the **scalar/geometry** half (MS2-S2) on the mature parquet/geoparquet path —
  ``manifest.json``, the root ``scalar_static.parquet`` rollup, per-basin
  ``scalar_dynamic.parquet``, and the root ``outlines.geoparquet`` — confirmed by
  :func:`hdx_fixtures.assertions.run_scalar_assertions`.
* the **gridded** half (MS2-S3) — per basin a ``gridded_static/<label>.tif``
  multiband COG and a ``gridded_dynamic/<label>.zarr`` Zarr v3 store sharing one
  aligned grid label, the Zarr time axis identical to the scalar ``time`` —
  confirmed by :func:`hdx_fixtures.assertions.run_gridded_assertions`.
* the **derived invalids** (MS2-S4 / MS8-S2) — ``invalid/wrong-format-version/``
  (pins M2), ``invalid/extra-manifest-field/`` (pins M3),
  ``invalid/empty-cadence/`` (pins M4), and ``invalid/missing-root-rollup/``
  (pins L1), each copied from the baseline and changed by exactly one surgical
  mutation (LOW-2), confirmed by
  :func:`hdx_fixtures.assertions.run_invalid_assertions`. Iterating the
  :class:`~hdx_fixtures.mutate.Invalid` enum, this loop derives every invalid the
  enum declares, so widening the enum (as MS8-S2 did) needs no change here.

The gridded half is emitted **after** the scalar half so the Zarr time axis can
align to the already-written scalar ``time`` (T2); the invalids are derived
**after** the full four-quadrant baseline exists (LOW-2: one mutation off a
complete, known-good tree).
"""

import argparse
import sys
from pathlib import Path

from hdx_fixtures import configure_logging, get_logger
from hdx_fixtures.assertions import (
    run_gridded_assertions,
    run_invalid_assertions,
    run_scalar_assertions,
)
from hdx_fixtures.grids import write_grids
from hdx_fixtures.manifest import write_manifest
from hdx_fixtures.mutate import Invalid, derive_invalid, invalid_root
from hdx_fixtures.outlines import write_outlines
from hdx_fixtures.scalar import write_scalar


def valid_minimal_root(repo_root: Path) -> Path:
    """Return the ``conformance/valid/minimal/`` dataset root under ``repo_root``."""
    return repo_root / "conformance" / "valid" / "minimal"


def build_scalar_baseline(dataset_root: Path) -> None:
    """Emit the scalar/geometry artifacts into ``dataset_root`` (spec §4)."""
    log = get_logger("build")
    log.info("building scalar baseline at %s", dataset_root)
    write_manifest(dataset_root)
    write_scalar(dataset_root)
    write_outlines(dataset_root)
    log.info("scalar baseline emitted")


def build_gridded_baseline(dataset_root: Path) -> None:
    """Emit the gridded COG + Zarr artifacts into ``dataset_root`` (spec §7/§8)."""
    log = get_logger("build")
    log.info("building gridded baseline at %s", dataset_root)
    write_grids(dataset_root)
    log.info("gridded baseline emitted")


def derive_invalids(dataset_root: Path) -> None:
    """Derive every invalid from the baseline + self-assert each (MS2-S4 / LOW-2).

    The repo root is recovered from ``dataset_root`` (``<repo>/conformance/valid/
    minimal``) so each invalid lands under ``<repo>/conformance/invalid/<name>/``.
    For every :class:`Invalid`, the baseline is copied and exactly one surgical
    mutation is applied, then the "differs in exactly one way" self-assertion runs
    (aborting on failure) before the next invalid. The loop iterates the
    :class:`Invalid` enum, so a widened enum (MS8-S2 added M3/M4) needs no edit.
    """
    log = get_logger("build")
    # dataset_root == <repo>/conformance/valid/minimal -> repo is three up.
    repo_root = dataset_root.parents[2]
    for invalid in Invalid:
        log.info("deriving invalid %s (pins %s)", invalid.value, invalid.pinned_check)
        derive_invalid(dataset_root, repo_root, invalid)
        run_invalid_assertions(
            dataset_root, invalid_root(repo_root, invalid), invalid
        )
    log.info("all invalids derived + self-assertions passed")


def main(argv: list[str] | None = None) -> int:
    """Emit the four-quadrant baseline and run its self-assertions (abort on failure)."""
    parser = argparse.ArgumentParser(
        description="Build the MS2 valid baseline (scalar + gridded halves)."
    )
    parser.add_argument(
        "--dataset-root",
        type=Path,
        required=True,
        help="conformance/valid/minimal/ dataset root to write into",
    )
    args = parser.parse_args(argv)

    configure_logging()
    log = get_logger("build")

    dataset_root: Path = args.dataset_root
    build_scalar_baseline(dataset_root)
    run_scalar_assertions(dataset_root)
    build_gridded_baseline(dataset_root)
    run_gridded_assertions(dataset_root)
    derive_invalids(dataset_root)

    log.info("baseline + two derived invalids complete + self-assertions passed")
    # User-facing status line (output, not a diagnostic) — see architecture §2.
    print(
        f"conformance fixtures regenerated: valid baseline (four quadrants) at "
        f"{dataset_root}; two invalids derived under conformance/invalid/; "
        "all self-assertions passed"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
