"""Write the scalar half of the valid baseline (parquet, spec §4/§6/§8).

This emits the two scalar artifacts of the basin-first hive (spec §4):

* ``scalar_static.parquet`` at the dataset root — the dataset-level rollup: one
  row per basin, columns ``basin_id`` + at least one ``scalar·static`` field
  (spec §2 quadrant ``[]``). zstd-compressed.
* ``basin=<id>/scalar_dynamic.parquet`` per basin — rows = ``time``, columns
  ``basin_id`` + at least one ``scalar·dynamic`` field (quadrant ``[T]``). The
  ``time`` column is a **full timestamp**, **non-nullable**, **sorted ascending**
  (spec §6 / T1), written **with row-group statistics on ``time``** (spec §8),
  zstd-compressed.

The basins are deliberately **ragged across basins** in their period of record
(spec §6.1) while each basin's own axis is internally consistent. In-file
``basin_id`` equals the ``basin=<id>`` folder for every basin (spec §3 / I2) and
``basin_id`` values are unique across basins (I3).

This module emits only the *writer-intended* bytes; the writer self-assertions
in :mod:`hdx_fixtures.assertions` re-open the files and confirm the engineered
properties (including the MED-5 row-group-statistics hand-off).

Field names are opaque producer strings (spec §2): the static field is
``drainage_area`` and the dynamic field is ``streamflow``; HDX attaches no role,
units magic, or belongs-to link to either.
"""

import datetime as dt
from dataclasses import dataclass
from pathlib import Path

import pyarrow as pa
import pyarrow.parquet as pq

from hdx_fixtures import get_logger

# Parquet compression mandated for scalar artifacts (spec §8).
_COMPRESSION = "zstd"

# Microsecond-precision timestamp: a full timestamp (date+time), the harmless
# superset HDX mandates for all datasets (spec §6, resolved open question 1).
TIME_TYPE = pa.timestamp("us")

# Opaque producer-chosen field names (spec §2). Distinct quadrants:
STATIC_FIELD = "drainage_area"  # scalar·static, f64
DYNAMIC_FIELD = "streamflow"  # scalar·dynamic, f64


@dataclass(frozen=True)
class BasinSpec:
    """A basin's identity and its (ragged) scalar period of record.

    The ``time`` axis is built as one timestamp per day for ``n_days`` starting
    at ``start`` — distinct ``(start, n_days)`` per basin yields the
    ragged-across-basins extents of spec §6.1.
    """

    basin_id: str
    drainage_area: float
    start: dt.datetime
    n_days: int


# The valid baseline's basins. ≥2 basins (spec exit criterion), with deliberately
# DIFFERENT (start, n_days) so their time extents are ragged across basins
# (spec §6.1). basin_id values are unique (I3). Folder names derive from basin_id.
BASINS: tuple[BasinSpec, ...] = (
    BasinSpec("0001", 1234.5, dt.datetime(2000, 1, 1), 5),
    BasinSpec("0002", 6789.0, dt.datetime(2010, 6, 15), 7),
    BasinSpec("0003", 42.0, dt.datetime(2005, 3, 1), 4),
)


def basin_dir(dataset_root: Path, basin_id: str) -> Path:
    """Return the ``basin=<id>`` partition folder for ``basin_id`` (spec §4)."""
    return dataset_root / f"basin={basin_id}"


def _time_axis(basin: BasinSpec) -> list[dt.datetime]:
    """Build the sorted, daily, full-timestamp axis for ``basin`` (spec §6)."""
    return [basin.start + dt.timedelta(days=i) for i in range(basin.n_days)]


def _dynamic_table(basin: BasinSpec) -> pa.Table:
    """Build the ``scalar_dynamic`` table for one basin (spec §6 schema).

    The ``time`` column is declared **non-nullable** in the Arrow schema (T1) and
    populated with a sorted-ascending daily axis. The dynamic field is f64.
    """
    times = _time_axis(basin)
    # streamflow: a deterministic, finite, per-basin series (values are opaque).
    flow = [round(10.0 + i * 0.5, 3) for i in range(basin.n_days)]

    schema = pa.schema(
        [
            pa.field("basin_id", pa.string(), nullable=False),
            pa.field("time", TIME_TYPE, nullable=False),
            pa.field(DYNAMIC_FIELD, pa.float64(), nullable=True),
        ]
    )
    return pa.table(
        {
            "basin_id": pa.array([basin.basin_id] * basin.n_days, type=pa.string()),
            "time": pa.array(times, type=TIME_TYPE),
            DYNAMIC_FIELD: pa.array(flow, type=pa.float64()),
        },
        schema=schema,
    )


def write_scalar_dynamic(dataset_root: Path, basin: BasinSpec) -> Path:
    """Write ``basin=<id>/scalar_dynamic.parquet`` for one basin and return it.

    The file is written sorted by ``time`` with row-group statistics enabled
    (spec §8): the parquet ``SortingColumn`` metadata records the time sort, and
    ``write_statistics`` ensures per-row-group min/max on every column including
    ``time``. zstd compression per spec §8.
    """
    log = get_logger("scalar")
    folder = basin_dir(dataset_root, basin.basin_id)
    folder.mkdir(parents=True, exist_ok=True)
    path = folder / "scalar_dynamic.parquet"

    table = _dynamic_table(basin)
    time_index = table.schema.get_field_index("time")

    pq.write_table(
        table,
        path,
        compression=_COMPRESSION,
        write_statistics=True,
        sorting_columns=[
            pq.SortingColumn(
                column_index=time_index, descending=False, nulls_first=False
            )
        ],
    )

    log.info(
        "wrote scalar_dynamic basin=%s rows=%d span=%s..%s",
        basin.basin_id,
        basin.n_days,
        basin.start.date().isoformat(),
        (basin.start + dt.timedelta(days=basin.n_days - 1)).date().isoformat(),
    )
    return path


def write_scalar_static(dataset_root: Path) -> Path:
    """Write the root ``scalar_static.parquet`` rollup and return its path.

    One row per basin, columns ``basin_id`` + the ``scalar·static`` field
    ``drainage_area`` (f64). zstd-compressed with statistics (spec §4/§8).
    ``basin_id`` is non-nullable and unique across rows (I3).
    """
    log = get_logger("scalar")
    dataset_root.mkdir(parents=True, exist_ok=True)
    path = dataset_root / "scalar_static.parquet"

    schema = pa.schema(
        [
            pa.field("basin_id", pa.string(), nullable=False),
            pa.field(STATIC_FIELD, pa.float64(), nullable=True),
        ]
    )
    table = pa.table(
        {
            "basin_id": pa.array([b.basin_id for b in BASINS], type=pa.string()),
            STATIC_FIELD: pa.array(
                [b.drainage_area for b in BASINS], type=pa.float64()
            ),
        },
        schema=schema,
    )

    pq.write_table(table, path, compression=_COMPRESSION, write_statistics=True)

    log.info("wrote scalar_static rows=%d (1 row/basin)", len(BASINS))
    return path


def write_scalar(dataset_root: Path) -> list[Path]:
    """Write the root rollup + every per-basin ``scalar_dynamic``; return paths."""
    written = [write_scalar_static(dataset_root)]
    written.extend(write_scalar_dynamic(dataset_root, basin) for basin in BASINS)
    return written
