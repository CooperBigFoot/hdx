"""Scalar-side writer self-assertions (abort generation on failure).

These assertions re-open the *written* fixture bytes and confirm the engineered
properties the writers (:mod:`hdx_fixtures.scalar`, :mod:`hdx_fixtures.outlines`,
:mod:`hdx_fixtures.manifest`) intended. They are **load-bearing**: every public
``assert_*`` raises :class:`AssertionFailed` on violation, and
:func:`run_scalar_assertions` propagates that so ``regenerate.sh`` aborts with a
non-zero exit — a broken property never produces a committed fixture.

These are **writer-side** assertions (Python). They are distinct from the
Rust-side enforcement in ``validate`` (spec §10): they assert what the *writer*
intended, not what a *Rust reader* recovers. See the MED-5 hand-off note on
:func:`assert_time_column_and_statistics`.
"""

import json
from pathlib import Path

import pyarrow as pa
import pyarrow.parquet as pq

from hdx_fixtures import get_logger
from hdx_fixtures.manifest import MANIFEST_FIELDS, build_manifest
from hdx_fixtures.outlines import DELINEATION_ALT, DELINEATION_PRIMARY
from hdx_fixtures.scalar import (
    BASINS,
    DYNAMIC_FIELD,
    STATIC_FIELD,
    basin_dir,
)


class AssertionFailed(RuntimeError):
    """Raised when a writer self-assertion fails on the emitted bytes.

    Fires whenever the re-opened fixture does not exhibit an engineered property
    (e.g. an unsorted ``time`` column, a missing row-group statistic, a
    basin_id/folder disagreement). Propagating it aborts generation so a broken
    fixture is never committed.
    """


def _require(condition: bool, message: str) -> None:
    """Raise :class:`AssertionFailed` with ``message`` unless ``condition``."""
    if not condition:
        raise AssertionFailed(message)


def _read_table(path: Path) -> pa.Table:
    """Read a parquet file as its *true* file schema (no hive inference).

    ``pq.read_table`` infers a ``basin`` partition column from the enclosing
    ``basin=<id>`` folder and appends it — a read-time artifact, not in the file.
    Passing ``partitioning=None`` reads exactly the columns the writer emitted, so
    the self-assertions inspect the real on-disk schema (spec §3/§6).
    """
    return pq.read_table(path, partitioning=None)


def assert_time_column_and_statistics(dataset_root: Path) -> None:
    # --- MED-5 WRITER/READER HAND-OFF (spec §8; planning/MS2/steps.md) ---------
    # This is a WRITER-side assertion: it confirms the *written* parquet carries
    # a non-nullable, sorted-ascending, full-timestamp `time` column AND usable
    # per-row-group min/max statistics on `time`. It proves what THIS writer
    # emitted — it cannot prove what the Rust reader recovers.
    #
    # MS3 MUST confirm from the Rust side (`arrow`/`parquet`) that the time
    # extent is sourced from these row-group statistics (not a bounded-scan
    # fallback) on this valid fixture. If MS3 finds the Rust reader cannot
    # recover them, the fix is to REGENERATE the fixture (adjust this generator
    # and re-emit) — NEVER a reader workaround. A mismatch is a generator bug.
    # ---------------------------------------------------------------------------
    """Confirm each ``scalar_dynamic`` ``time`` column is conformant + stats-carrying.

    For every basin, re-open ``scalar_dynamic.parquet`` and assert ``time`` is
    named ``time``, a full-timestamp logical type, non-nullable, sorted ascending
    with no nulls (spec §6 / T1), and that the file metadata exposes usable
    per-row-group min/max statistics on ``time`` (spec §8).
    """
    log = get_logger("assert.time")
    for basin in BASINS:
        path = basin_dir(dataset_root, basin.basin_id) / "scalar_dynamic.parquet"
        _require(path.exists(), f"missing scalar_dynamic for basin {basin.basin_id}")

        table = _read_table(path)
        schema = table.schema

        _require("time" in schema.names, f"{path}: no `time` column")
        time_field = schema.field("time")
        _require(
            pa.types.is_timestamp(time_field.type),
            f"{path}: `time` is {time_field.type}, expected a full timestamp",
        )
        _require(
            not time_field.nullable,
            f"{path}: `time` is nullable; spec §6/T1 requires non-nullable",
        )

        time_col = table.column("time")
        _require(time_col.null_count == 0, f"{path}: `time` has nulls")

        values = time_col.to_pylist()
        _require(
            all(a <= b for a, b in zip(values, values[1:], strict=False)),
            f"{path}: `time` is not sorted ascending",
        )

        # Re-read file metadata: row-group statistics on `time` (spec §8 / MED-5).
        meta = pq.read_metadata(path)
        time_index = schema.get_field_index("time")
        saw_stats = False
        for rg_idx in range(meta.num_row_groups):
            col = meta.row_group(rg_idx).column(time_index)
            _require(
                col.is_stats_set,
                f"{path}: row group {rg_idx} has no statistics on `time`",
            )
            stats = col.statistics
            _require(
                stats is not None
                and stats.min is not None
                and stats.max is not None,
                f"{path}: row group {rg_idx} `time` stats missing min/max",
            )
            _require(
                stats.min <= stats.max,
                f"{path}: row group {rg_idx} `time` min>max",
            )
            saw_stats = True
        _require(saw_stats, f"{path}: no row groups to carry `time` statistics")

        log.info(
            "time OK basin=%s rows=%d stats_min=%s stats_max=%s",
            basin.basin_id,
            table.num_rows,
            values[0],
            values[-1],
        )


def assert_basin_id_folder_agreement_and_unique(dataset_root: Path) -> None:
    """Confirm in-file ``basin_id`` == folder and ``basin_id`` is unique (I2/I3).

    For each basin, every ``basin_id`` value in ``scalar_dynamic.parquet`` equals
    the ``basin=<id>`` folder name (spec §3 / I2). Across the root
    ``scalar_static.parquet`` the ``basin_id`` set is unique with one row per
    basin (I3), and matches the basin folder set.
    """
    log = get_logger("assert.identity")

    folder_ids: list[str] = []
    for basin in BASINS:
        folder = basin_dir(dataset_root, basin.basin_id)
        path = folder / "scalar_dynamic.parquet"
        _require(path.exists(), f"missing scalar_dynamic for basin {basin.basin_id}")

        table = _read_table(path)
        ids = set(table.column("basin_id").to_pylist())
        _require(
            ids == {basin.basin_id},
            f"{path}: in-file basin_id {ids} disagrees with folder "
            f"basin={basin.basin_id} (spec §3/I2)",
        )
        folder_ids.append(basin.basin_id)

    static_path = dataset_root / "scalar_static.parquet"
    _require(static_path.exists(), "missing scalar_static.parquet at root")
    static = _read_table(static_path)
    static_ids = static.column("basin_id").to_pylist()
    _require(
        len(static_ids) == len(set(static_ids)),
        f"{static_path}: basin_id is not unique (spec §3/I3): {static_ids}",
    )
    _require(
        static.num_rows == len(BASINS),
        f"{static_path}: expected 1 row/basin ({len(BASINS)}), got {static.num_rows}",
    )
    _require(
        set(static_ids) == set(folder_ids),
        f"{static_path}: rollup basin_id set {set(static_ids)} != folders "
        f"{set(folder_ids)}",
    )
    _require(
        STATIC_FIELD in static.schema.names,
        f"{static_path}: missing static field `{STATIC_FIELD}`",
    )

    log.info(
        "identity OK basins=%d unique=%s", len(BASINS), sorted(set(static_ids))
    )


def assert_time_ragged_across_basins(dataset_root: Path) -> None:
    """Confirm basins' ``time`` extents differ across basins (spec §6.1).

    Reads each basin's ``time`` extent and asserts the set of ``(min, max)``
    extents has more than one distinct value — i.e. the dataset is ragged across
    basins (homogeneity is about fields, not time extent).
    """
    log = get_logger("assert.ragged")
    extents: set[tuple[object, object]] = set()
    for basin in BASINS:
        path = basin_dir(dataset_root, basin.basin_id) / "scalar_dynamic.parquet"
        values = _read_table(path).column("time").to_pylist()
        _require(len(values) > 0, f"{path}: empty `time` axis")
        extents.add((values[0], values[-1]))

    _require(
        len(extents) > 1,
        f"time extents are not ragged across basins (spec §6.1): {extents}",
    )
    log.info("ragged-across-basins OK distinct_extents=%d", len(extents))


def assert_dynamic_field_present(dataset_root: Path) -> None:
    """Confirm each ``scalar_dynamic`` carries the dynamic field (spec §2)."""
    log = get_logger("assert.dynamic")
    for basin in BASINS:
        path = basin_dir(dataset_root, basin.basin_id) / "scalar_dynamic.parquet"
        names = _read_table(path).schema.names
        _require(
            DYNAMIC_FIELD in names,
            f"{path}: missing dynamic field `{DYNAMIC_FIELD}`",
        )
    log.info("dynamic field `%s` present in every basin", DYNAMIC_FIELD)


def assert_outlines(dataset_root: Path) -> None:
    """Confirm ``outlines.geoparquet`` schema, plurality, single-file (Geo1).

    Asserts the columns are exactly ``(basin_id, delineation, geometry)``, that
    at least one basin carries ≥2 distinct ``delineation`` labels (spec §9
    plurality), and that outlines are a single root file (not a partitioned
    directory).
    """
    log = get_logger("assert.outlines")
    path = dataset_root / "outlines.geoparquet"
    _require(path.exists(), "missing outlines.geoparquet at root")
    _require(path.is_file(), f"{path}: outlines must be a single file, not a dir")

    table = _read_table(path)
    cols = table.schema.names
    _require(
        cols == ["basin_id", "delineation", "geometry"],
        f"{path}: schema {cols} != (basin_id, delineation, geometry) (Geo1)",
    )

    labels = table.column("delineation").to_pylist()
    basin_ids = table.column("basin_id").to_pylist()
    _require(
        {DELINEATION_PRIMARY, DELINEATION_ALT}.issubset(set(labels)),
        f"{path}: expected neutral labels incl. "
        f"{DELINEATION_PRIMARY!r},{DELINEATION_ALT!r}; got {sorted(set(labels))}",
    )

    # ≥2 distinct delineation labels for at least one basin (spec §9 plurality).
    per_basin: dict[str, set[str]] = {}
    for bid, label in zip(basin_ids, labels, strict=True):
        per_basin.setdefault(bid, set()).add(label)
    _require(
        any(len(v) >= 2 for v in per_basin.values()),
        f"{path}: no basin has ≥2 delineation labels (spec §9 plurality): "
        f"{per_basin}",
    )

    log.info(
        "outlines OK rows=%d labels=%s plural_basin=%s",
        table.num_rows,
        sorted(set(labels)),
        next(bid for bid, v in per_basin.items() if len(v) >= 2),
    )


def assert_manifest_floor(dataset_root: Path) -> None:
    """Confirm ``manifest.json`` is exactly the six floor fields (spec §11).

    Re-reads the written manifest and asserts its key set is **exactly**
    :data:`MANIFEST_FIELDS` (no derivable seventh field, no missing field),
    matching the built object. ``format_version`` is the literal ``"0.1"``.
    """
    log = get_logger("assert.manifest")
    path = dataset_root / "manifest.json"
    _require(path.exists(), "missing manifest.json at root")

    obj = json.loads(path.read_text(encoding="utf-8"))
    _require(isinstance(obj, dict), f"{path}: manifest is not a JSON object")
    _require(
        set(obj.keys()) == set(MANIFEST_FIELDS),
        f"{path}: manifest keys {sorted(obj.keys())} != six floor fields "
        f"{sorted(MANIFEST_FIELDS)} (spec §11)",
    )
    _require(
        obj == build_manifest(),
        f"{path}: on-disk manifest differs from the built manifest object",
    )
    _require(
        obj["format_version"] == "0.1",
        f"{path}: format_version {obj['format_version']!r} != '0.1' (hard cut)",
    )
    log.info("manifest floor OK fields=%d", len(obj))


def run_scalar_assertions(dataset_root: Path) -> None:
    """Run every scalar-side self-assertion; raise on the first failure.

    Invoked by ``regenerate.sh`` after the scalar/outlines emit. Any
    :class:`AssertionFailed` propagates so generation aborts with a non-zero
    exit (the assertions are load-bearing).
    """
    log = get_logger("assert")
    assert_manifest_floor(dataset_root)
    assert_time_column_and_statistics(dataset_root)
    assert_basin_id_folder_agreement_and_unique(dataset_root)
    assert_time_ragged_across_basins(dataset_root)
    assert_dynamic_field_present(dataset_root)
    assert_outlines(dataset_root)
    log.info("all scalar self-assertions passed")
