"""Build the valid baseline (scalar + gridded halves), then self-assert (MS2-S3).

This is the generation entry point ``regenerate.sh`` calls. It emits the one
valid dataset into ``conformance/valid/minimal/`` in two halves and runs the
load-bearing self-assertions after each, aborting on any failure:

* the **scalar/geometry** half (MS2-S2) on the mature parquet/geoparquet path —
  ``manifest.json``, the root ``scalar_static.parquet`` rollup, per-basin
  ``scalar_dynamic.parquet``, and the root ``outlines.geoparquet`` — confirmed by
  :func:`hdx_fixtures.assertions.run_scalar_assertions`.
* the **gridded** half (MS2-S3) — per basin a ``gridded_static/<label>.tif``
  multiband COG and a ``gridded_dynamic/<label>.zarr`` Zarr v3 store sharing one
  aligned grid label, the Zarr time axis identical to the scalar ``time`` —
  confirmed by :func:`hdx_fixtures.assertions.run_gridded_assertions`.

The gridded half is emitted **after** the scalar half so the Zarr time axis can
align to the already-written scalar ``time`` (T2). Together they complete the
**four-quadrant** valid dataset; the two derived invalids are a later MS2 step.
"""

import argparse
import sys
from pathlib import Path

from hdx_fixtures import configure_logging, get_logger
from hdx_fixtures.assertions import run_gridded_assertions, run_scalar_assertions
from hdx_fixtures.grids import write_grids
from hdx_fixtures.manifest import write_manifest
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

    log.info("MS2-S3 four-quadrant baseline complete + self-assertions passed")
    # User-facing status line (output, not a diagnostic) — see architecture §2.
    print(f"MS2-S3: valid baseline (four quadrants) emitted at {dataset_root}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
