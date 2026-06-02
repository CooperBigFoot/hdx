"""Write the dataset-level ``outlines.geoparquet`` (spec §9 / Geo1).

Geometry ships *in* HDX as outline polygons in one **dataset-level**
``outlines.geoparquet`` rollup (spec §9, resolved open question 2) — **not**
per-basin, **not** partitioned by delineation. Rows are
``(basin_id, delineation, geometry)``; a basin's competing delineations sit
together, discernible by the ``delineation`` column.

Outlines are **plural** (spec §9): at least one basin carries **≥2 delineation
labels**. The labels are **neutral** producer strings (``merit`` / ``grit`` here)
— never trusted as a published "hydrofabric" source; HDX copies them in as
neutral labeled data and references no originating source (spec §13).

This writes a single file at the dataset root; the writer self-assertions in
:mod:`hdx_fixtures.assertions` confirm the schema, the ≥2-labels-for-≥1-basin
plurality, and that it is one (non-partitioned) file.
"""

from dataclasses import dataclass
from pathlib import Path

import geopandas as gpd
from shapely.geometry import Polygon

from hdx_fixtures import get_logger
from hdx_fixtures.manifest import CRS
from hdx_fixtures.scalar import BASINS

# Neutral delineation labels (spec §9) — opaque producer strings, no source magic.
DELINEATION_PRIMARY = "merit"
DELINEATION_ALT = "grit"


@dataclass(frozen=True)
class OutlineRow:
    """One ``(basin_id, delineation, geometry)`` outline row (spec §9)."""

    basin_id: str
    delineation: str
    geometry: Polygon


def _square(cx: float, cy: float, half: float = 0.05) -> Polygon:
    """Return a small axis-aligned square polygon centred at ``(cx, cy)``.

    Coordinates are in the dataset CRS (EPSG:4326 degrees); the exact shape is
    immaterial to HDX — only that a valid polygon geometry is present.
    """
    return Polygon(
        [
            (cx - half, cy - half),
            (cx + half, cy - half),
            (cx + half, cy + half),
            (cx - half, cy + half),
        ]
    )


def _outline_rows() -> list[OutlineRow]:
    """Build the outline rows: one ``merit`` per basin, plus a ``grit`` for the
    first basin so it carries ≥2 delineation labels (spec §9 plurality / Geo1).
    """
    rows: list[OutlineRow] = []
    for i, basin in enumerate(BASINS):
        cx = float(i)
        rows.append(
            OutlineRow(basin.basin_id, DELINEATION_PRIMARY, _square(cx, 0.0))
        )
    # Give the first basin a second, slightly-offset delineation (plurality).
    first = BASINS[0]
    rows.append(OutlineRow(first.basin_id, DELINEATION_ALT, _square(0.02, 0.02)))
    return rows


def write_outlines(dataset_root: Path) -> Path:
    """Write the root ``outlines.geoparquet`` and return its path (spec §9)."""
    log = get_logger("outlines")
    dataset_root.mkdir(parents=True, exist_ok=True)
    path = dataset_root / "outlines.geoparquet"

    rows = _outline_rows()
    gdf = gpd.GeoDataFrame(
        {
            "basin_id": [r.basin_id for r in rows],
            "delineation": [r.delineation for r in rows],
        },
        geometry=[r.geometry for r in rows],
        crs=CRS,
    )
    # Column order: (basin_id, delineation, geometry) per spec §9 / Geo1.
    gdf = gdf[["basin_id", "delineation", "geometry"]]
    gdf.to_parquet(path)

    log.info(
        "wrote outlines.geoparquet rows=%d delineations=%s",
        len(rows),
        sorted({r.delineation for r in rows}),
    )
    return path
