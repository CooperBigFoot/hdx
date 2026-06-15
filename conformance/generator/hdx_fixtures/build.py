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
* the **derived invalids** (MS2-S4 / MS8) — the manifest/layout negatives
  ``invalid/wrong-format-version/`` (M2), ``invalid/extra-manifest-field/`` (M3),
  ``invalid/empty-cadence/`` (M4), ``invalid/missing-root-rollup/`` (L1); the
  MS8-S3 Bucket-B parquet/layout negatives (I1/I2/H1/T1/L2); and the MS8-S2
  georef/grid-label negatives ``invalid/crs-mismatch/`` (M5),
  ``invalid/misaligned-shared-label/`` (G2), ``invalid/divergent-grid-label-set/``
  (H2) — each copied from the baseline and changed by exactly one surgical
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

import json

from hdx_fixtures import configure_logging, get_logger
from hdx_fixtures.assertions import (
    _require,
    run_gridded_assertions,
    run_invalid_assertions,
    run_multi_grid_multi_static_assertions,
    run_scalar_assertions,
)
from hdx_fixtures.grids import write_grids, write_multi_family_grids
from hdx_fixtures.manifest import (
    FORMAT_VERSION_V0_2,
    MANIFEST_FIELDS,
    build_manifest,
    write_manifest,
)
from hdx_fixtures.mutate import Invalid, derive_invalid, invalid_root
from hdx_fixtures.outlines import write_outlines
from hdx_fixtures.scalar import write_scalar


def valid_minimal_root(repo_root: Path) -> Path:
    """Return the ``conformance/valid/minimal/`` dataset root under ``repo_root``."""
    return repo_root / "conformance" / "valid" / "minimal"


def valid_geometry_less_root(repo_root: Path) -> Path:
    """Return the ``conformance/valid/geometry-less/`` dataset root under ``repo_root``.

    The geometry-optional 0.2 fixture: the four-quadrant baseline **minus**
    ``outlines.geoparquet``, with a ``format_version "0.2"`` manifest. ``validate``
    must report it conformant under 0.2 (the L1 outlines leg is skipped) and
    non-conformant under 0.1 (the leg fires).
    """
    return repo_root / "conformance" / "valid" / "geometry-less"


def valid_multi_grid_multi_static_root(repo_root: Path) -> Path:
    """Return the ``conformance/valid/multi_grid_multi_static/`` dataset root.

    The merge-gen M1 field-catalog-completeness fixture: the four-quadrant
    baseline with TWO DISTINCT grid labels per gridded quadrant
    (``dem``+``landcover`` gridded·static, ``era5``+``merit`` gridded·dynamic),
    each label carrying its own distinctly-named field. The gridded field catalog
    must union fields across BOTH families, so ``describe`` surfaces every family's
    field — the end-to-end M1 proof. ``validate`` reports it conformant (the two
    static + two dynamic labels are homogeneous across basins and georeferenced).
    """
    return repo_root / "conformance" / "valid" / "multi_grid_multi_static"


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


def build_multi_grid_multi_static_baseline(dataset_root: Path) -> None:
    """Emit the merge-gen M1 two-family baseline (spec §4/§7/§8).

    A full four-quadrant dataset like :func:`build_scalar_baseline` +
    :func:`build_gridded_baseline`, except the gridded half carries TWO DISTINCT
    grid labels per quadrant (``dem``+``landcover`` static, ``era5``+``merit``
    dynamic) via :func:`~hdx_fixtures.grids.write_multi_family_grids`. The scalar
    half, outlines, and 0.1 manifest are the baseline shape, so the only axis that
    differs is the multi-family gridded catalog — the field-catalog-completeness
    proof surface.
    """
    log = get_logger("build")
    log.info("building multi_grid_multi_static baseline at %s", dataset_root)
    write_manifest(dataset_root)
    write_scalar(dataset_root)
    write_outlines(dataset_root)
    write_multi_family_grids(dataset_root)
    log.info("multi_grid_multi_static baseline emitted")


def build_geometry_less_baseline(dataset_root: Path) -> None:
    """Emit the geometry-optional 0.2 baseline: the scalar baseline WITHOUT outlines.

    This is :func:`build_scalar_baseline` minus the
    :func:`~hdx_fixtures.outlines.write_outlines` step, with a ``format_version
    "0.2"`` manifest — a **pure-scalar** dataset (the agent-materializer's shape,
    FUSION_ARC FIRST_SLICE): ``manifest.json`` + ``scalar_static.parquet`` (the L1
    floor, the basin-set source-of-truth under 0.2) + per-basin
    ``scalar_dynamic.parquet``, but NO ``outlines.geoparquet`` and NO gridded
    subtrees. Under 0.2 the absent outlines is a skipped L1 / Geo1 leg
    (conformant); under 0.1 the L1 outlines leg fires (non-conformant). With no
    gridded·dynamic geometry, ``describe`` carries empty ``delineations``, present
    scalar time extents, and NO ``gridded_time_axis`` (the additive gridded field
    is omitted for a pure-scalar basin).
    """
    log = get_logger("build")
    log.info("building geometry-less (0.2) scalar baseline at %s", dataset_root)
    write_manifest(dataset_root, format_version=FORMAT_VERSION_V0_2)
    write_scalar(dataset_root)
    log.info("geometry-less (0.2) baseline emitted (no outlines, no gridded)")


def run_geometry_less_assertions(dataset_root: Path) -> None:
    """Self-assert the geometry-less 0.2 fixture; raise on the first failure.

    Confirms the load-bearing geometry-optional shape: ``scalar_static.parquet``
    present, ``outlines.geoparquet`` ABSENT, and a six-field ``format_version
    "0.2"`` manifest. The shared scalar/gridded self-assertions reused here are
    geometry-independent; the outlines-specific ``assert_outlines`` is
    deliberately NOT run (this fixture has none).
    """
    log = get_logger("assert")

    # The geometry-optional invariant: scalar_static present, outlines absent,
    # and (pure-scalar) no gridded subtrees in any basin.
    _require(
        (dataset_root / "scalar_static.parquet").exists(),
        "geometry-less: scalar_static.parquet must be present (the L1 floor)",
    )
    _require(
        not (dataset_root / "outlines.geoparquet").exists(),
        "geometry-less: outlines.geoparquet must be ABSENT (the 0.2 relaxation)",
    )
    gridded_dirs = sorted(
        p.as_posix()
        for p in dataset_root.glob("basin=*/gridded_*")
    )
    _require(
        not gridded_dirs,
        f"geometry-less: pure-scalar fixture must carry NO gridded subtrees, "
        f"found {gridded_dirs}",
    )
    basins = sorted(p.name for p in dataset_root.glob("basin=*"))
    _require(
        bool(basins),
        "geometry-less: must carry per-basin scalar_dynamic basins",
    )
    for basin in basins:
        _require(
            (dataset_root / basin / "scalar_dynamic.parquet").exists(),
            f"geometry-less: {basin} must carry scalar_dynamic.parquet",
        )

    # The manifest is the six floor fields with format_version "0.2".
    manifest_path = dataset_root / "manifest.json"
    obj = json.loads(manifest_path.read_text(encoding="utf-8"))
    _require(
        set(obj.keys()) == set(MANIFEST_FIELDS),
        f"geometry-less: manifest keys {sorted(obj.keys())} != six floor fields",
    )
    _require(
        obj == build_manifest(format_version=FORMAT_VERSION_V0_2),
        "geometry-less: on-disk manifest differs from the built 0.2 manifest object",
    )
    _require(
        obj["format_version"] == FORMAT_VERSION_V0_2,
        f"geometry-less: format_version {obj['format_version']!r} != '0.2'",
    )

    log.info("geometry-less (0.2) self-assertions passed")


def derive_invalids(dataset_root: Path) -> None:
    """Derive every fixture from the baseline + self-assert each (MS2-S4 / LOW-2).

    The repo root is recovered from ``dataset_root`` (``<repo>/conformance/valid/
    minimal``) so each fixture lands under its :func:`~hdx_fixtures.mutate.invalid_root`
    location — every variant is a fail-closed invalid under HDX 0.2, under
    ``<repo>/conformance/invalid/<name>/`` (the former still-conformant
    :attr:`~hdx_fixtures.mutate.Invalid.IRREGULAR_TIME_AXIS` case is now a fail-closed
    M6 rule-(b) negative). For every
    :class:`Invalid`, the baseline is copied and exactly one surgical mutation is
    applied, then the "differs in exactly one way" self-assertion runs (aborting on
    failure) before the next fixture. The loop iterates the :class:`Invalid` enum,
    so a widened enum (MS8-S2/S3 added variants) needs no edit.
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

    # The geometry-optional 0.2 fixture lives under valid/geometry-less/, a sibling
    # of valid/minimal/ (dataset_root == <repo>/conformance/valid/minimal).
    repo_root = dataset_root.parents[2]
    geometry_less = valid_geometry_less_root(repo_root)
    build_geometry_less_baseline(geometry_less)
    run_geometry_less_assertions(geometry_less)

    # The merge-gen M1 two-family fixture lives under valid/multi_grid_multi_static/,
    # a sibling of valid/minimal/: two distinct grid labels per gridded quadrant so
    # the field catalog must union fields across families (the M1 completeness proof).
    multi = valid_multi_grid_multi_static_root(repo_root)
    build_multi_grid_multi_static_baseline(multi)
    run_multi_grid_multi_static_assertions(multi)

    log.info(
        "baseline + derived fixtures + geometry-less + multi_grid_multi_static "
        "complete + self-assertions passed"
    )
    # User-facing status line (output, not a diagnostic) — see architecture §2.
    print(
        f"conformance fixtures regenerated: valid baseline (four quadrants) at "
        f"{dataset_root}; geometry-less (0.2, no outlines) at {geometry_less}; "
        f"multi_grid_multi_static (two grid families) at {multi}; "
        "fail-closed invalids derived under conformance/invalid/ "
        "(including the irregular-time-axis M6 rule-(b) negative); "
        "all self-assertions passed"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
