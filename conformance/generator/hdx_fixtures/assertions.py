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

import datetime as dt
import json
from pathlib import Path

import numpy as np
import pyarrow as pa
import pyarrow.parquet as pq
import rasterio
import zarr

from hdx_fixtures import get_logger
from hdx_fixtures.grids import (
    COMPANION_MASK_FIELD,
    EPSG_CODE,
    GRID_LABEL,
    GRIDDED_DYNAMIC_FIELD,
    GRIDDED_STATIC_FIELD,
    _time_since_epoch,
    cog_path,
    zarr_path,
)
from hdx_fixtures.manifest import MANIFEST_FIELDS, build_manifest
from hdx_fixtures.mutate import (
    EMPTY_CADENCE,
    EXTRA_MANIFEST_FIELD,
    EXTRA_MANIFEST_FIELD_VALUE,
    H1_DIVERGENT_FIELD,
    I2_FOREIGN_BASIN_ID,
    MISSING_ROOT_ROLLUP,
    WRONG_FORMAT_VERSION,
    Invalid,
    _MUTATED_BASIN,
)
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


# --- gridded-side helpers ----------------------------------------------------


def _scalar_time_days(dataset_root: Path, basin_id: str) -> list[int]:
    """Return a basin's scalar ``time`` axis as CF integer days-since-epoch.

    Re-reads ``scalar_dynamic.parquet`` and encodes its ``time`` column the same
    way the Zarr writer does (:func:`hdx_fixtures.grids._time_since_epoch`), so
    the intra-basin alignment assertion compares like with like (spec §6.2/T2).
    """
    path = basin_dir(dataset_root, basin_id) / "scalar_dynamic.parquet"
    values: list[dt.datetime] = _read_table(path).column("time").to_pylist()
    return _time_since_epoch(values).tolist()


def _cog_grid(path: Path) -> dict[str, object]:
    """Return the COG's grid signature (CRS, affine, extent, resolution)."""
    with rasterio.open(path) as src:
        return {
            "crs": src.crs.to_epsg(),
            "transform": tuple(round(v, 9) for v in tuple(src.transform)[:6]),
            "width": src.width,
            "height": src.height,
            "res": tuple(round(v, 9) for v in src.res),
            "bounds": tuple(round(v, 9) for v in tuple(src.bounds)),
        }


def _zarr_grid(store: Path) -> dict[str, object]:
    """Return the Zarr's grid signature derived from its CF ``lat``/``lon``.

    Reconstructs the cell size, extent and a GeoTIFF-equivalent affine from the
    explicit ``lat``/``lon`` coordinate arrays so it can be compared cell-for-cell
    with the COG's georeferencing (spec §8 shared-label alignment).
    """
    group = zarr.open_group(str(store), mode="r")
    lat = np.asarray(group["lat"][:], dtype="float64")
    lon = np.asarray(group["lon"][:], dtype="float64")
    res_x = round(float(lon[1] - lon[0]), 9)
    res_y = round(float(lat[0] - lat[1]), 9)
    width = int(lon.shape[0])
    height = int(lat.shape[0])
    # lat/lon are cell centers; recover the north-west corner (top-left edge).
    west = round(float(lon[0]) - res_x / 2.0, 9)
    north = round(float(lat[0]) + res_y / 2.0, 9)
    south = round(north - res_y * height, 9)
    east = round(west + res_x * width, 9)
    transform = (res_x, 0.0, west, 0.0, -res_y, north)
    return {
        "transform": tuple(round(v, 9) for v in transform),
        "width": width,
        "height": height,
        "res": (res_x, res_y),
        "bounds": (west, south, east, north),
    }


def assert_shared_grid_label_and_alignment(dataset_root: Path) -> None:
    """Confirm the COG and Zarr per basin share a label and are aligned (G2).

    For every basin: the ``gridded_static`` COG and ``gridded_dynamic`` Zarr both
    live under the single shared :data:`hdx_fixtures.grids.GRID_LABEL` (spec §8 —
    a shared label across the subtrees signals alignment), and their grid
    signatures (CRS, affine, extent, resolution, bounds) are **cell-for-cell**
    identical — the G2 positive-path precondition.
    """
    log = get_logger("assert.grid")
    for basin in BASINS:
        bdir = basin_dir(dataset_root, basin.basin_id)
        cog = cog_path(bdir)
        store = zarr_path(bdir)
        _require(cog.exists(), f"missing gridded_static COG for basin {basin.basin_id}")
        _require(store.exists(), f"missing gridded_dynamic Zarr for basin {basin.basin_id}")

        # Shared grid label: both artifacts are named `<GRID_LABEL>.{tif,zarr}`.
        _require(
            cog.stem == GRID_LABEL and store.stem == GRID_LABEL,
            f"basin {basin.basin_id}: COG/Zarr labels {cog.stem!r}/{store.stem!r} "
            f"!= shared label {GRID_LABEL!r} (spec §8)",
        )

        cog_grid = _cog_grid(cog)
        _require(
            cog_grid["crs"] == EPSG_CODE,
            f"{cog}: CRS EPSG:{cog_grid['crs']} != EPSG:{EPSG_CODE} (spec §7.4)",
        )
        zarr_grid = _zarr_grid(store)
        for key in ("transform", "width", "height", "res", "bounds"):
            _require(
                cog_grid[key] == zarr_grid[key],
                f"basin {basin.basin_id}: COG/Zarr {key} mismatch — "
                f"{cog_grid[key]!r} != {zarr_grid[key]!r} (G2 alignment, spec §8)",
            )

        log.info(
            "grid OK basin=%s label=%s %dx%d res=%s bounds=%s",
            basin.basin_id,
            GRID_LABEL,
            cog_grid["height"],
            cog_grid["width"],
            cog_grid["res"],
            cog_grid["bounds"],
        )


def assert_cog_self_naming_and_georef(dataset_root: Path) -> None:
    """Confirm each COG self-names its bands and carries georef tags (G1/G3).

    For every basin: the COG band description equals the gridded·static field
    name (no positional channel axis, G1), the file carries standard GeoTIFF
    georeferencing tags (a CRS and an affine transform, G3), is internally tiled
    with overviews (§8), and is dense ``[Y,X]`` over the bbox (§7.1).
    """
    log = get_logger("assert.cog")
    for basin in BASINS:
        cog = cog_path(basin_dir(dataset_root, basin.basin_id))
        with rasterio.open(cog) as src:
            descs = list(src.descriptions)
            _require(
                GRIDDED_STATIC_FIELD in descs,
                f"{cog}: band descriptions {descs} miss field "
                f"`{GRIDDED_STATIC_FIELD}` (self-naming, G1)",
            )
            _require(
                src.crs is not None and src.crs.to_epsg() == EPSG_CODE,
                f"{cog}: missing/unexpected CRS {src.crs} (G3 / spec §7.4)",
            )
            _require(
                src.transform is not None and not src.transform.is_identity,
                f"{cog}: missing affine georeferencing transform (G3)",
            )
            _require(src.is_tiled, f"{cog}: COG is not internally tiled (spec §8)")
            _require(
                len(src.overviews(1)) >= 1,
                f"{cog}: COG has no overviews (spec §8)",
            )
            _require(
                src.height > 0 and src.width > 0,
                f"{cog}: empty grid (spec §7.1 dense rectangular)",
            )
        log.info("cog OK basin=%s band=%s", basin.basin_id, GRIDDED_STATIC_FIELD)


def assert_zarr_self_naming_and_cf_georef(dataset_root: Path) -> None:
    """Confirm each Zarr self-names variables with CF georef (G1/G3, spec §7.3).

    For every basin: the gridded·dynamic field and its companion mask are named
    CF variables (no positional channel axis, G1); explicit ``lat``/``lon``
    coordinate arrays and a ``grid_mapping``/CRS variable are present (CF georef,
    G3); each variable references the CRS via ``grid_mapping``; arrays are dense
    ``[T,Y,X]`` (spec §7.1); and the store is Zarr v3.
    """
    log = get_logger("assert.zarr")
    for basin in BASINS:
        store = zarr_path(basin_dir(dataset_root, basin.basin_id))
        group = zarr.open_group(str(store), mode="r")
        names = set(group.array_keys())

        for required in (GRIDDED_DYNAMIC_FIELD, COMPANION_MASK_FIELD):
            _require(
                required in names,
                f"{store}: missing CF variable `{required}` (self-naming, G1)",
            )
        for coord in ("time", "lat", "lon"):
            _require(coord in names, f"{store}: missing CF coordinate `{coord}` (G3)")
        _require("crs" in names, f"{store}: missing grid_mapping/CRS variable (G3)")

        crs_attrs = dict(group["crs"].attrs)
        _require(
            crs_attrs.get("spatial_ref") == f"EPSG:{EPSG_CODE}",
            f"{store}: crs spatial_ref {crs_attrs.get('spatial_ref')!r} "
            f"!= EPSG:{EPSG_CODE} (spec §7.4)",
        )

        var = group[GRIDDED_DYNAMIC_FIELD]
        _require(
            int(var.metadata.zarr_format) == 3,
            f"{store}: `{GRIDDED_DYNAMIC_FIELD}` is not Zarr v3 (spec §8)",
        )
        _require(
            var.ndim == 3,
            f"{store}: `{GRIDDED_DYNAMIC_FIELD}` is {var.ndim}-D, expected "
            f"[T,Y,X] (spec §7.1)",
        )
        _require(
            dict(var.attrs).get("grid_mapping") == "crs",
            f"{store}: `{GRIDDED_DYNAMIC_FIELD}` missing grid_mapping=crs (G3)",
        )
        log.info(
            "zarr CF OK basin=%s vars=%s",
            basin.basin_id,
            [GRIDDED_DYNAMIC_FIELD, COMPANION_MASK_FIELD],
        )


def assert_zarr_time_matches_scalar(dataset_root: Path) -> None:
    """Confirm each Zarr ``time`` axis is identical to the scalar ``time`` (T2).

    For every basin: the Zarr ``time`` coordinate (CF integer days-since-epoch)
    equals the basin's ``scalar_dynamic.parquet`` ``time`` column under the same
    encoding (spec §6.2 / T2). The Zarr ``time`` is also CF-encoded
    (``units``/``calendar`` attributes present).
    """
    log = get_logger("assert.time2")
    for basin in BASINS:
        store = zarr_path(basin_dir(dataset_root, basin.basin_id))
        group = zarr.open_group(str(store), mode="r")
        zarr_time = [int(v) for v in np.asarray(group["time"][:]).tolist()]
        time_attrs = dict(group["time"].attrs)
        _require(
            "units" in time_attrs and "calendar" in time_attrs,
            f"{store}: `time` missing CF units/calendar (spec §6.3)",
        )
        scalar_time = _scalar_time_days(dataset_root, basin.basin_id)
        _require(
            zarr_time == scalar_time,
            f"basin {basin.basin_id}: Zarr time {zarr_time} != scalar time "
            f"{scalar_time} (intra-basin alignment, spec §6.2/T2)",
        )
        log.info(
            "time2 OK basin=%s n=%d span=%d..%d",
            basin.basin_id,
            len(zarr_time),
            zarr_time[0],
            zarr_time[-1],
        )


def assert_zarr_time_ragged_across_basins(dataset_root: Path) -> None:
    """Confirm Zarr ``time`` extents still differ across basins (spec §6.1).

    Mirrors the scalar ragged-across check on the gridded side: the set of Zarr
    ``(min, max)`` time extents has more than one distinct value, so the
    intra-basin alignment (T2) did not flatten the ragged-across-basins property.
    """
    log = get_logger("assert.ragged2")
    extents: set[tuple[int, int]] = set()
    for basin in BASINS:
        store = zarr_path(basin_dir(dataset_root, basin.basin_id))
        group = zarr.open_group(str(store), mode="r")
        vals = [int(v) for v in np.asarray(group["time"][:]).tolist()]
        _require(len(vals) > 0, f"{store}: empty Zarr `time` axis")
        extents.add((vals[0], vals[-1]))
    _require(
        len(extents) > 1,
        f"Zarr time extents not ragged across basins (spec §6.1): {extents}",
    )
    log.info("zarr ragged-across-basins OK distinct_extents=%d", len(extents))


def assert_zarr_consolidated_and_sharded(dataset_root: Path) -> None:
    # --- MED-5 WRITER/READER HAND-OFF (spec §8; planning/MS2/steps.md) ---------
    # This is a WRITER-side assertion: it confirms the *written* Zarr store
    # carries v3 consolidated metadata (the root `zarr.json` holds the store's
    # consolidated_metadata) and uses v3 sharding (a ShardingCodec on the data
    # variable). It proves what THIS writer emitted — not what the Rust reader
    # recovers.
    #
    # MS4 MUST confirm from the Rust side (`zarrs`) that it reads the store's
    # metadata via the §8 consolidated path (or explicitly classify it an R3
    # byte-deep skip with a stated reason). If MS4 finds the Rust reader cannot
    # recover consolidated metadata, the fix is to REGENERATE the fixture
    # (adjust this generator and re-emit) — NEVER a reader workaround. A
    # mismatch is a generator bug. See conformance/README.md (Rule 3).
    # ---------------------------------------------------------------------------
    """Confirm each Zarr store has consolidated metadata + v3 sharding (spec §8).

    For every basin: the root ``zarr.json`` carries a ``consolidated_metadata``
    block (one GET learns the store), and the gridded·dynamic data variable is
    encoded with a Zarr v3 sharding codec (sane S3 object counts at 50k basins).
    """
    log = get_logger("assert.zarr.meta")
    for basin in BASINS:
        store = zarr_path(basin_dir(dataset_root, basin.basin_id))
        root_meta = store / "zarr.json"
        _require(root_meta.is_file(), f"{store}: missing root zarr.json (spec §8)")

        meta = json.loads(root_meta.read_text(encoding="utf-8"))
        _require(
            meta.get("consolidated_metadata") is not None,
            f"{store}: no consolidated_metadata in root zarr.json (spec §8)",
        )

        group = zarr.open_group(str(store), mode="r")
        codecs = [type(c).__name__ for c in group[GRIDDED_DYNAMIC_FIELD].metadata.codecs]
        _require(
            any("Sharding" in name for name in codecs),
            f"{store}: `{GRIDDED_DYNAMIC_FIELD}` has no v3 sharding codec "
            f"(codecs={codecs}, spec §8)",
        )
        log.info(
            "zarr meta OK basin=%s consolidated=yes sharded=yes",
            basin.basin_id,
        )


def assert_gridded_fields_ordinary(dataset_root: Path) -> None:
    """Confirm the {source}_{variable} + companion-mask fields are ordinary (§2).

    For every basin: the ``{source}_{variable}`` field (``era5_precipitation``)
    and the ``{field}_was_filled`` companion mask are present as **ordinary**
    Zarr variables — same array kind, no special attribute marks a role or a
    belongs-to link (the generator attaches no suffix/prefix magic). This proves
    the patterns get no privileged handling (spec §2).
    """
    log = get_logger("assert.ordinary")
    forbidden = {"role", "belongs_to", "quadrant", "kind", "semantic", "provenance"}
    for basin in BASINS:
        store = zarr_path(basin_dir(dataset_root, basin.basin_id))
        group = zarr.open_group(str(store), mode="r")
        names = set(group.array_keys())
        _require(
            GRIDDED_DYNAMIC_FIELD in names,
            f"{store}: missing {{source}}_{{variable}} field "
            f"`{GRIDDED_DYNAMIC_FIELD}`",
        )
        _require(
            COMPANION_MASK_FIELD in names,
            f"{store}: missing companion-mask field `{COMPANION_MASK_FIELD}`",
        )
        for field in (GRIDDED_DYNAMIC_FIELD, COMPANION_MASK_FIELD):
            attrs = set(dict(group[field].attrs).keys())
            leaked = attrs & forbidden
            _require(
                not leaked,
                f"{store}: `{field}` carries non-ordinary attrs {leaked} "
                f"(inert/agnostic, spec §2)",
            )
        log.info(
            "ordinary OK basin=%s source_variable=%s companion_mask=%s",
            basin.basin_id,
            GRIDDED_DYNAMIC_FIELD,
            COMPANION_MASK_FIELD,
        )


def assert_four_quadrants_present(dataset_root: Path) -> None:
    """Confirm the dataset now spans all four field quadrants (spec §2).

    Checks one artifact of each physical encoding exists for the first basin:
    ``scalar_static.parquet`` (scalar·static), ``scalar_dynamic.parquet``
    (scalar·dynamic), the ``gridded_static`` COG (gridded·static), and the
    ``gridded_dynamic`` Zarr (gridded·dynamic) — a true mix-quadrant dataset.
    """
    log = get_logger("assert.quadrants")
    first = BASINS[0]
    bdir = basin_dir(dataset_root, first.basin_id)
    checks = {
        "scalar·static": dataset_root / "scalar_static.parquet",
        "scalar·dynamic": bdir / "scalar_dynamic.parquet",
        "gridded·static": cog_path(bdir),
        "gridded·dynamic": zarr_path(bdir),
    }
    for quadrant, path in checks.items():
        _require(path.exists(), f"quadrant {quadrant} missing artifact at {path}")
    log.info("four quadrants present: %s", sorted(checks.keys()))


def run_gridded_assertions(dataset_root: Path) -> None:
    """Run every gridded-side self-assertion; raise on the first failure.

    Invoked by ``regenerate.sh`` after the gridded emit (MS2-S3). Any
    :class:`AssertionFailed` propagates so generation aborts with a non-zero
    exit (the assertions are load-bearing). Confirms shared-label alignment (G2),
    CF/GeoTIFF self-naming + georef (G1/G3), intra-basin time alignment (T2) with
    ragged-across-basins still holding (§6.1), Zarr consolidated metadata + v3
    sharding (§8; the MED-5 MS4 hand-off), the ordinary ``{source}_{variable}`` +
    companion-mask fields (§2), and that all four quadrants are present.
    """
    log = get_logger("assert")
    assert_shared_grid_label_and_alignment(dataset_root)
    assert_cog_self_naming_and_georef(dataset_root)
    assert_zarr_self_naming_and_cf_georef(dataset_root)
    assert_zarr_time_matches_scalar(dataset_root)
    assert_zarr_time_ragged_across_basins(dataset_root)
    assert_zarr_consolidated_and_sharded(dataset_root)
    assert_gridded_fields_ordinary(dataset_root)
    assert_four_quadrants_present(dataset_root)
    log.info("all gridded self-assertions passed")


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


# --- invalid-side self-assertion (LOW-2: differs in exactly one way) ----------


def _relative_files(tree_root: Path) -> set[str]:
    """Return every dataset file in ``tree_root`` as POSIX paths relative to root.

    Walks the whole tree (Zarr stores are directories of many chunk/metadata
    files, so a recursive walk is required) and returns only files — directories
    are implied by their contents. Used to diff the file *set* of two trees.

    The trees no longer contain goldens (they are committed under
    ``conformance/goldens/``, outside the gitignored fixture trees), so the
    "differs in exactly one way" diff is purely about dataset bytes.
    """
    return {
        p.relative_to(tree_root).as_posix()
        for p in tree_root.rglob("*")
        if p.is_file()
    }


def _changed_files(baseline_root: Path, invalid_root: Path) -> set[str]:
    """Return relative paths present in BOTH trees whose bytes differ.

    Only inspects files common to both trees; added/removed files are reported
    separately by the file-set diff. Byte comparison is exact so a single mutated
    value (e.g. ``format_version``) surfaces as exactly one changed file.
    """
    common = _relative_files(baseline_root) & _relative_files(invalid_root)
    changed: set[str] = set()
    for rel in common:
        base_bytes = (baseline_root / rel).read_bytes()
        inv_bytes = (invalid_root / rel).read_bytes()
        if base_bytes != inv_bytes:
            changed.add(rel)
    return changed


def _mutated_dynamic_rel() -> str:
    """Return the mutated basin's ``scalar_dynamic.parquet`` POSIX-relative path."""
    return f"basin={_MUTATED_BASIN}/scalar_dynamic.parquet"


def _assert_one_parquet_mutation(
    invalid_root: Path, invalid: Invalid, changed: set[str]
) -> None:
    """Confirm a Bucket-B parquet rewrite touched exactly one file, correctly.

    For each parquet-rewrite variant (I1/I2/H1/T1): exactly one shared file
    differs (the mutated artifact), and re-reading it confirms the single intended
    content divergence. Raises :class:`AssertionFailed` otherwise (LOW-2 one
    surgical mutation, proven from the emitted bytes).
    """
    expected_file = _mutated_dynamic_rel()
    _require(
        changed == {expected_file},
        f"{invalid_root}: differs in {sorted(changed)}, expected only "
        f"{{{expected_file!r}}} (LOW-2 one mutation, pins {invalid.pinned_check})",
    )

    mutated = _read_table(invalid_root / expected_file)
    names = mutated.schema.names

    if invalid is Invalid.MISSING_BASIN_ID_COLUMN:
        # I1: the mutated scalar_dynamic no longer carries the basin_id column.
        _require(
            "basin_id" not in names,
            f"{invalid_root}: {expected_file} still has a basin_id column "
            f"(I1 requires it dropped)",
        )
        _require(
            DYNAMIC_FIELD in names and "time" in names,
            f"{invalid_root}: {expected_file} lost more than basin_id "
            f"(LOW-2: only basin_id may be dropped)",
        )
    elif invalid is Invalid.BASIN_ID_FOLDER_MISMATCH:
        # I2: the in-file basin_id is the foreign value, disagreeing with the folder
        # yet still a single unique value (so I3 stays pass).
        ids = set(mutated.column("basin_id").to_pylist())
        _require(
            ids == {I2_FOREIGN_BASIN_ID},
            f"{invalid_root}: {expected_file} in-file basin_id {ids} != "
            f"{{{I2_FOREIGN_BASIN_ID!r}}} (I2 folder mismatch, kept unique)",
        )
        _require(
            I2_FOREIGN_BASIN_ID != _MUTATED_BASIN,
            f"{invalid_root}: foreign basin_id {I2_FOREIGN_BASIN_ID!r} equals the "
            f"folder id (I2 needs a disagreement)",
        )
    elif invalid is Invalid.RAGGED_FIELD_SCHEMA:
        # H1: the data field is renamed; only the name diverges (dtype unchanged).
        _require(
            H1_DIVERGENT_FIELD in names and DYNAMIC_FIELD not in names,
            f"{invalid_root}: {expected_file} fields {names} did not rename "
            f"{DYNAMIC_FIELD!r} → {H1_DIVERGENT_FIELD!r} (H1 ragged schema)",
        )
        _require(
            pa.types.is_floating(mutated.schema.field(H1_DIVERGENT_FIELD).type),
            f"{invalid_root}: {expected_file} renamed field dtype changed "
            f"(LOW-2: only the name may diverge for H1)",
        )
    else:  # Invalid.NON_MONOTONIC_TIME
        # T1: the time column is descending (first value exceeds the last), the
        # only divergence; the column stays named time, timestamp, non-nullable.
        time_field = mutated.schema.field("time")
        _require(
            pa.types.is_timestamp(time_field.type) and not time_field.nullable,
            f"{invalid_root}: {expected_file} time is no longer a non-nullable "
            f"timestamp (LOW-2: only the sort order may diverge for T1)",
        )
        values = mutated.column("time").to_pylist()
        _require(
            len(values) >= 2 and values[0] > values[-1],
            f"{invalid_root}: {expected_file} time is not descending {values} "
            f"(T1 non-monotonic)",
        )


def assert_differs_in_exactly_one_way(
    baseline_root: Path, invalid_root: Path, invalid: Invalid
) -> None:
    """Confirm ``invalid_root`` is exactly one surgical mutation off the baseline.

    Enforces the LOW-2 one-mutation invariant at generation time via a recursive
    tree diff against the valid baseline (spec §10 / R2; see
    ``conformance/README.md`` Rule 2):

    * :attr:`Invalid.WRONG_FORMAT_VERSION` — the trees have the **same file set**;
      exactly **one** file (``manifest.json``) differs, and it differs **only** in
      the ``format_version`` value (``"0.1"`` → ``"0.2"``), every other manifest
      field byte-identical (pins **M2**).
    * :attr:`Invalid.EXTRA_MANIFEST_FIELD` — the trees have the **same file set**;
      exactly **one** file (``manifest.json``) differs, and its key set is the six
      floor fields **plus** the one extra :data:`EXTRA_MANIFEST_FIELD` key (whose
      value is :data:`EXTRA_MANIFEST_FIELD_VALUE`); the six floor values are
      byte-identical to the baseline (pins **M3**, the six-field floor, spec
      §0/§11).
    * :attr:`Invalid.EMPTY_CADENCE` — the trees have the **same file set**;
      exactly **one** file (``manifest.json``) differs, its key set is exactly the
      six floor fields, and only the ``cadence`` value differs (now the empty
      string :data:`EMPTY_CADENCE`); the other five floor values are byte-identical
      (pins **M4**, ``cadence`` non-empty, spec §6.4 / §11 — an entry-gate ``Err``
      that rejects the empty cadence before ``check_m6`` rule (a)).
    * :attr:`Invalid.MISSING_ROOT_ROLLUP` — the invalid tree is missing **exactly
      one** file (``outlines.geoparquet``), adds nothing, and **no** shared file's
      bytes differ — the rest of the tree is byte-identical (pins **L1**).

    The **Bucket-B** parquet/layout negatives (MS8-S3) each touch exactly one
    basin's artifact, adding nothing:

    * :attr:`Invalid.MISSING_BASIN_ID_COLUMN` — exactly **one** file (the mutated
      basin's ``scalar_dynamic.parquet``) differs; re-read, it no longer carries a
      ``basin_id`` column, while the baseline still does (pins **I1**).
    * :attr:`Invalid.BASIN_ID_FOLDER_MISMATCH` — exactly **one** file (the mutated
      basin's ``scalar_dynamic.parquet``) differs; its in-file ``basin_id`` set is
      exactly ``{I2_FOREIGN_BASIN_ID}`` (``9999``), disagreeing with the
      ``basin=<id>`` folder yet unique across the dataset (pins **I2**).
    * :attr:`Invalid.RAGGED_FIELD_SCHEMA` — exactly **one** file (the mutated
      basin's ``scalar_dynamic.parquet``) differs; its data field is renamed
      ``streamflow`` → :data:`H1_DIVERGENT_FIELD` (``flow``), the only schema
      divergence (pins **H1**).
    * :attr:`Invalid.NON_MONOTONIC_TIME` — exactly **one** file (the mutated
      basin's ``scalar_dynamic.parquet``) differs; its ``time`` column is
      descending (the first value exceeds the last), the only divergence (pins
      **T1**).
    * :attr:`Invalid.MISSING_GRIDDED_DYNAMIC_SUBTREE` — the invalid tree is missing
      **exactly** the mutated basin's ``gridded_dynamic/`` subtree (every removed
      path lies under it), adds nothing, and **no** shared file's bytes differ
      (pins **L2**).
    """
    log = get_logger("assert.invalid")
    base_files = _relative_files(baseline_root)
    inv_files = _relative_files(invalid_root)

    added = inv_files - base_files
    removed = base_files - inv_files
    _require(
        not added,
        f"{invalid_root}: invalid adds files not in baseline {sorted(added)} "
        f"(LOW-2: one surgical mutation only)",
    )

    # The three manifest-mutation invalids share the same shape invariant: the file
    # set is unchanged and exactly `manifest.json` differs. Their per-key checks
    # then diverge below (M2 value / M3 extra key / M4 empty cadence).
    manifest_only = {
        Invalid.WRONG_FORMAT_VERSION,
        Invalid.EXTRA_MANIFEST_FIELD,
        Invalid.EMPTY_CADENCE,
    }

    if invalid in manifest_only:
        _require(
            not removed,
            f"{invalid_root}: {invalid.value} removed files {sorted(removed)}; "
            f"the only change must be the manifest (LOW-2)",
        )
        changed = _changed_files(baseline_root, invalid_root)
        _require(
            changed == {"manifest.json"},
            f"{invalid_root}: differs in {sorted(changed)}, expected only "
            f"{{'manifest.json'}} (LOW-2 one mutation)",
        )

        base_manifest = json.loads(
            (baseline_root / "manifest.json").read_text(encoding="utf-8")
        )
        inv_manifest = json.loads(
            (invalid_root / "manifest.json").read_text(encoding="utf-8")
        )

        if invalid is Invalid.WRONG_FORMAT_VERSION:
            _require(
                set(inv_manifest.keys()) == set(MANIFEST_FIELDS),
                f"{invalid_root}: manifest keys {sorted(inv_manifest.keys())} != "
                f"six floor fields (the mutation must not add/remove fields)",
            )
            _require(
                inv_manifest["format_version"] == WRONG_FORMAT_VERSION,
                f"{invalid_root}: format_version {inv_manifest['format_version']!r} "
                f"!= {WRONG_FORMAT_VERSION!r} (M2 hard cut)",
            )
            differing_keys = {
                key
                for key in MANIFEST_FIELDS
                if base_manifest.get(key) != inv_manifest.get(key)
            }
            _require(
                differing_keys == {"format_version"},
                f"{invalid_root}: manifest differs in {sorted(differing_keys)}, "
                f"expected only {{'format_version'}} (LOW-2 one mutation)",
            )
        elif invalid is Invalid.EXTRA_MANIFEST_FIELD:
            # M3: the key set is the six floor fields PLUS exactly one extra key.
            _require(
                set(inv_manifest.keys())
                == set(MANIFEST_FIELDS) | {EXTRA_MANIFEST_FIELD},
                f"{invalid_root}: manifest keys {sorted(inv_manifest.keys())} != "
                f"six floor fields + {{{EXTRA_MANIFEST_FIELD!r}}} (M3 7th field)",
            )
            _require(
                inv_manifest.get(EXTRA_MANIFEST_FIELD) == EXTRA_MANIFEST_FIELD_VALUE,
                f"{invalid_root}: extra field {EXTRA_MANIFEST_FIELD!r} = "
                f"{inv_manifest.get(EXTRA_MANIFEST_FIELD)!r} != "
                f"{EXTRA_MANIFEST_FIELD_VALUE!r} (M3)",
            )
            # The six floor values are unchanged: the ONLY difference is the added
            # key (one surgical mutation, LOW-2).
            differing_floor = {
                key
                for key in MANIFEST_FIELDS
                if base_manifest.get(key) != inv_manifest.get(key)
            }
            _require(
                not differing_floor,
                f"{invalid_root}: floor fields {sorted(differing_floor)} changed; "
                f"the only change must be the added key {EXTRA_MANIFEST_FIELD!r} "
                f"(LOW-2 one mutation)",
            )
        else:  # Invalid.EMPTY_CADENCE
            # M4: the key set is exactly the six floor fields; only `cadence` differs.
            _require(
                set(inv_manifest.keys()) == set(MANIFEST_FIELDS),
                f"{invalid_root}: manifest keys {sorted(inv_manifest.keys())} != "
                f"six floor fields (the mutation must not add/remove fields)",
            )
            _require(
                inv_manifest["cadence"] == EMPTY_CADENCE,
                f"{invalid_root}: cadence {inv_manifest['cadence']!r} != "
                f"{EMPTY_CADENCE!r} (M4 non-empty cadence)",
            )
            differing_keys = {
                key
                for key in MANIFEST_FIELDS
                if base_manifest.get(key) != inv_manifest.get(key)
            }
            _require(
                differing_keys == {"cadence"},
                f"{invalid_root}: manifest differs in {sorted(differing_keys)}, "
                f"expected only {{'cadence'}} (LOW-2 one mutation)",
            )
    elif invalid is Invalid.MISSING_ROOT_ROLLUP:
        _require(
            removed == {MISSING_ROOT_ROLLUP},
            f"{invalid_root}: missing files {sorted(removed)}, expected exactly "
            f"{{{MISSING_ROOT_ROLLUP!r}}} (LOW-2 one mutation, pins L1)",
        )
        changed = _changed_files(baseline_root, invalid_root)
        _require(
            not changed,
            f"{invalid_root}: shared files differ {sorted(changed)}; removing one "
            f"rollup must leave the rest byte-identical (LOW-2)",
        )
    elif invalid is Invalid.MISSING_GRIDDED_DYNAMIC_SUBTREE:
        # L2: the whole gridded_dynamic/ subtree of exactly the mutated basin is
        # removed (a Zarr store is many files), nothing is added, and no shared file
        # differs. Every removed path lies under that one basin's gridded_dynamic/.
        subtree_prefix = (
            f"basin={_MUTATED_BASIN}/gridded_dynamic/"
        )
        _require(
            removed and all(rel.startswith(subtree_prefix) for rel in removed),
            f"{invalid_root}: removed files {sorted(removed)} are not exactly the "
            f"{subtree_prefix!r} subtree (LOW-2 one mutation, pins L2)",
        )
        changed = _changed_files(baseline_root, invalid_root)
        _require(
            not changed,
            f"{invalid_root}: shared files differ {sorted(changed)}; deleting one "
            f"basin's gridded_dynamic subtree must leave the rest byte-identical "
            f"(LOW-2)",
        )
    else:
        # The Bucket-B parquet mutations (I1/I2/I3/H1/T1) each rewrite exactly one
        # parquet file: no file added or removed, exactly one shared file differs.
        _require(
            not removed,
            f"{invalid_root}: {invalid.value} removed files {sorted(removed)}; a "
            f"parquet rewrite must add/remove nothing (LOW-2)",
        )
        changed = _changed_files(baseline_root, invalid_root)
        _assert_one_parquet_mutation(invalid_root, invalid, changed)

    log.info(
        "invalid OK name=%s pins=%s added=%d removed=%d (one surgical mutation)",
        invalid.value,
        invalid.pinned_check,
        len(added),
        len(removed),
    )


def run_invalid_assertions(baseline_root: Path, invalid_root: Path, invalid: Invalid) -> None:
    """Run the invalid-side self-assertion; raise on failure.

    Invoked by ``regenerate.sh`` (via :mod:`hdx_fixtures.build`) after each
    invalid is derived (MS2-S4). Any :class:`AssertionFailed` propagates so
    generation aborts with a non-zero exit (the assertion is load-bearing): an
    invalid that differs from the baseline in more than the one intended way is
    never committed.
    """
    log = get_logger("assert")
    assert_differs_in_exactly_one_way(baseline_root, invalid_root, invalid)
    log.info("invalid self-assertion passed for %s", invalid.value)
