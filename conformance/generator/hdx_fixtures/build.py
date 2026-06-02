"""Build the valid baseline's scalar/geometry half, then self-assert (MS2-S2).

This is the generation entry point ``regenerate.sh`` calls for MS2-S2. It emits
the *scalar* half of the one valid dataset onto the mature parquet/geoparquet
path — ``manifest.json``, the root ``scalar_static.parquet`` rollup, per-basin
``scalar_dynamic.parquet``, and the root ``outlines.geoparquet`` — into
``conformance/valid/minimal/``, then runs the load-bearing scalar self-assertions
(:func:`hdx_fixtures.assertions.run_scalar_assertions`), aborting on any failure.

The gridded half (COG + Zarr) and the derived invalids are later MS2 steps; this
step emits a **partial-but-valid** scalar tree whose every artifact is conformant
on its own terms.
"""

import argparse
import sys
from pathlib import Path

from hdx_fixtures import configure_logging, get_logger
from hdx_fixtures.assertions import run_scalar_assertions
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


def main(argv: list[str] | None = None) -> int:
    """Emit the scalar baseline and run its self-assertions (abort on failure)."""
    parser = argparse.ArgumentParser(description="Build the MS2 valid baseline (scalar half).")
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

    log.info("MS2-S2 scalar baseline complete + self-assertions passed")
    # User-facing status line (output, not a diagnostic) — see architecture §2.
    print(f"MS2-S2: valid baseline scalar + outlines emitted at {dataset_root}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
