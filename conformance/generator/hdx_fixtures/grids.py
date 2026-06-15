"""Write the gridded half of the valid baseline (COG + Zarr, spec §7/§8).

This emits the two *gridded* artifacts of the basin-first hive (spec §4), per
basin, into the **same** ``basin=<id>/`` partition the scalar half (S2) wrote:

* ``gridded_static/<label>.tif`` — a multiband COG (quadrant ``[Y,X]``). Each
  **band description = field name** (no positional channel axis, G1), with
  standard GeoTIFF georeferencing tags (CRS + affine, G3), internal tiling +
  overviews (§8). The single ``gridded·static`` field is ``elevation`` (f32).
* ``gridded_dynamic/<label>.zarr`` — a Zarr **v3** store (quadrant ``[T,Y,X]``).
  Each **named CF variable = field name** (no positional channel axis, G1), with
  explicit ``lat``/``lon`` coordinate arrays + a ``grid_mapping``/CRS variable
  (CF georef, G3), a ``time`` coordinate as CF integer-since-epoch, time-major
  chunking, **v3 sharding**, **consolidated metadata**, and blosc-zstd
  compression (§8). The ``gridded·dynamic`` field is ``era5_precipitation``
  (the ``{source}_{variable}`` pattern) plus an ordinary companion-mask field
  ``era5_precipitation_was_filled`` (the ``{field}_was_filled`` pattern).

**Shared grid label ⇒ alignment (spec §8).** The COG and the Zarr in each basin
use **one and the same** ``<label>`` and are written from the **same affine /
extent / resolution**, so they are **cell-for-cell aligned** (the G2
positive-path precondition). The literal per-basin extent lives in each file.

**Intra-basin time alignment (spec §6.2 / T2).** The Zarr ``time`` coordinate is
the *identical* axis (same timestamps) as that basin's
``scalar_dynamic.parquet`` ``time``. Gaps a field does not natively cover are
**NaN-filled** (here the first timestep of ``era5_precipitation`` is NaN and the
companion mask marks it), proving the §6.2 NaN-fill convention without any
special handling of the suffix.

**Delineation-neutral (spec §9).** Both artifacts are dense rectangular over the
basin bbox — never clipped or NaN'd to an outline. The grid is delineation
agnostic; clipping is a downstream op, out of HDX scope (§10).

Field names are opaque producer strings (spec §2). The ``{source}_{variable}``
and companion-mask patterns appear **only to prove later milestones give them no
special handling** — this module attaches no role, belongs-to link, or magic.

This module emits only the *writer-intended* bytes; the gridded self-assertions
in :mod:`hdx_fixtures.assertions` re-open the files and confirm the engineered
properties (including the MED-5 consolidated-metadata hand-off).
"""

import datetime as dt
import json
from dataclasses import dataclass
from pathlib import Path

import numpy as np
import rasterio
import zarr
from rasterio.crs import CRS as RasterioCRS
from rasterio.enums import Resampling
from rasterio.transform import from_origin
from zarr.codecs import BloscCodec, BloscShuffle

from hdx_fixtures import get_logger
from hdx_fixtures.manifest import CRS
from hdx_fixtures.scalar import BASINS, BasinSpec, _time_axis

# One shared grid label used by BOTH the COG and the Zarr in every basin. A
# shared label across the gridded_static and gridded_dynamic subtrees signals
# cell-for-cell alignment without opening either file (spec §8). It names the
# grid *family*; the literal per-basin extent/affine lives in each file.
GRID_LABEL = "era5"

# A DIVERGENT grid label one basin's COG+Zarr can be re-emitted under so that
# basin's grid-label set becomes ``{era5b}`` while every other basin's is
# ``{era5}`` — the H2 cross-basin label-set negative (MS8-S2,
# divergent-grid-label-set). Renaming BOTH subtrees keeps the shared-label
# COG+Zarr coinciding under ``era5b`` (G2 stays pass for that basin); only H2
# (cross-basin label-set equality) fires. See
# :func:`hdx_fixtures.mutate._mutate_divergent_grid_label_set`.
DIVERGENT_GRID_LABEL = "era5b"

# Opaque producer-chosen gridded field names (spec §2). Distinct quadrants:
GRIDDED_STATIC_FIELD = "elevation"  # gridded·static COG band, f32
# gridded·dynamic Zarr variable, f32, using the {source}_{variable} pattern:
GRIDDED_DYNAMIC_FIELD = "era5_precipitation"
# Ordinary companion-mask field ({field}_was_filled). HDX gives the suffix no
# magic and parses no belongs-to link (spec §2) — it is just another variable.
COMPANION_MASK_FIELD = "era5_precipitation_was_filled"

# --- multi_grid_multi_static seam (merge-gen M1) ------------------------------
#
# The merge-gen field-catalog-completeness proof needs a dataset carrying TWO
# DISTINCT grid labels per quadrant so the gridded field catalog must union
# fields across families (not just the first artifact). Every basin emits:
#
#   gridded_static/<label>.tif   under labels  dem, landcover  (two COGs)
#   gridded_dynamic/<label>.zarr under labels  era5, merit     (two Zarrs)
#
# The static label set {dem, landcover} and the dynamic label set {era5, merit}
# are disjoint, so no label is shared across subtrees: G2 (which only compares a
# shared label's COG+Zarr extents) finds no pair to compare and passes trivially.
# Each label carries its OWN, distinctly-named field so a first-artifact-only
# catalog would surface only ONE static + ONE dynamic field's family — the RED.
# Every basin carries the SAME four labels, so H2 (cross-basin label-set
# equality) stays pass. The grid geometry is the shared baseline geometry, so all
# four labels are georeferenced (G3) over the same extent.
#
# Field names are opaque producer strings (spec §2): the {source}_{variable}
# dynamic pattern and the companion-mask suffix carry NO special handling — they
# appear here only as ordinary, distinct catalog members.

# Two distinct gridded·static labels (each a single-band COG), distinct fields:
SECOND_STATIC_LABELS: tuple[tuple[str, str], ...] = (
    ("dem", "dem_elevation"),  # (grid_label, band field name), f32
    ("landcover", "landcover_class"),  # (grid_label, band field name), f32
)

# Two distinct gridded·dynamic labels (each a Zarr v3 store). Each carries its own
# {source}_{variable} data field plus its ordinary {field}_was_filled companion
# mask — so the catalog must union four dynamic fields across the two families.
SECOND_DYNAMIC_LABELS: tuple[tuple[str, str], ...] = (
    ("era5", "era5_precipitation"),  # (grid_label, data-var field name), f32
    ("merit", "merit_flow_accumulation"),  # (grid_label, data-var field name), f32
)

# Dataset-wide CRS (spec §7.4 / §11). EPSG:4326, the same value the manifest
# declares; M5 cross-checks the manifest CRS against the CRS carried in files.
# Derive the integer EPSG code FROM the manifest CRS string so the two cannot
# drift (the gridded files and manifest carry one dataset-wide CRS).
_EPSG_PREFIX = "EPSG:"
if not CRS.startswith(_EPSG_PREFIX):  # pragma: no cover - guards a constant
    raise ValueError(f"manifest CRS {CRS!r} is not an EPSG code; spec §7.4")
EPSG_CODE = int(CRS.removeprefix(_EPSG_PREFIX))

# The shared per-basin grid geometry (spec §7.1: dense rectangular over the
# bbox). Small dense grids keep the committed fixture tiny while remaining a
# real, georeferenced raster. Resolution is in CRS units (EPSG:4326 degrees).
GRID_HEIGHT = 8  # Y (rows)
GRID_WIDTH = 6  # X (cols)
GRID_RES = 0.25  # degrees per cell (square cells)
# North-west origin of the (shared) grid in degrees. The exact location is
# immaterial to HDX; only that the COG and Zarr agree on it cell-for-cell.
GRID_WEST = 10.0
GRID_NORTH = 50.0

# A MISALIGNED west origin (a half-cell shift east of :data:`GRID_WEST`) used by
# the misaligned-shared-label negative (MS8-S2, G2). Re-emitting ONLY one basin's
# COG at this origin — leaving its Zarr at the baseline geometry — keeps the
# shared ``era5`` label in both subtrees (H2 stays pass: the label set is still
# ``{era5}``) but breaks cell-for-cell alignment (extent/bounds diverge), so
# check_g2 ran:fails for that basin. The width/res/height are unchanged so the
# COG stays a valid, georeferenced raster (G3 stays pass). See
# :func:`hdx_fixtures.mutate._mutate_misaligned_shared_label`.
MISALIGNED_GRID_WEST = 10.5

# COG internal tiling block size. Kept small so the tiny fixture raster still
# tiles and supports overviews (spec §8: internal tiling + overviews).
COG_BLOCK = 16

# Zarr chunking: time-major (a [t-n, t] read is one contiguous range, spec §8).
# Chunks subdivide the shard so v3 sharding is meaningful (one shard holds many
# chunks → sane S3 object counts at 50k basins).
ZARR_TIME_CHUNK = 1
ZARR_Y_CHUNK = GRID_HEIGHT
ZARR_X_CHUNK = GRID_WIDTH

# CF time encoding (spec §6.3): integer days since the epoch, proleptic
# Gregorian — matching the manifest cadence "daily".
TIME_UNITS = "days since 1970-01-01"
TIME_CALENDAR = "proleptic_gregorian"
_EPOCH = dt.datetime(1970, 1, 1)


@dataclass(frozen=True)
class GridGeometry:
    """The shared affine/extent/resolution of one basin's grid (spec §7/§8).

    Both the COG and the Zarr in a basin are built from this single geometry, so
    they are cell-for-cell aligned (G2). ``west``/``north`` is the grid's
    north-west origin; cells march east (+lon) and south (-lat).
    """

    height: int
    width: int
    res: float
    west: float
    north: float

    @property
    def transform(self) -> object:
        """Return the GeoTIFF affine transform (north-up, square cells)."""
        return from_origin(self.west, self.north, self.res, self.res)

    def lon_centers(self) -> np.ndarray:
        """Return the X (lon) cell-center coordinates (CF ``lon`` array)."""
        return self.west + (np.arange(self.width) + 0.5) * self.res

    def lat_centers(self) -> np.ndarray:
        """Return the Y (lat) cell-center coordinates (CF ``lat`` array).

        Latitudes descend from ``north`` because the raster is north-up (row 0
        is the northern edge), so the COG band rows and the Zarr ``lat`` axis
        index the same cells in the same order.
        """
        return self.north - (np.arange(self.height) + 0.5) * self.res


def _stabilize_consolidated_metadata(store_path: Path) -> None:
    """Sort the consolidated-metadata members in the root ``zarr.json`` (determinism).

    ``zarr.consolidate_metadata`` collects the store's array/group members into the
    root ``zarr.json``'s ``consolidated_metadata.metadata`` map in an
    **implementation-defined order** (it iterates members via a set/dict whose
    iteration order varies run-to-run). That non-determinism would make the
    committed Zarr ``zarr.json`` differ on every regenerate even though the bytes
    are otherwise identical. This rewrites the root ``zarr.json`` with the member
    map re-serialized in a stable, sorted-by-name order so a regenerate is
    **byte-deterministic** (the milestones.md MS2 determinism criterion). It
    touches only member *ordering* within the consolidated map — no metadata value
    is changed — so the store stays equivalent for every reader.
    """
    root_meta = store_path / "zarr.json"
    obj = json.loads(root_meta.read_text(encoding="utf-8"))
    consolidated = obj.get("consolidated_metadata")
    if isinstance(consolidated, dict):
        members = consolidated.get("metadata")
        if isinstance(members, dict):
            consolidated["metadata"] = {key: members[key] for key in sorted(members)}
    # Match the rest of the store's serialization: 2-space indent (zarr-python's
    # default) so only the member ordering — not whitespace — is normalized.
    root_meta.write_text(json.dumps(obj, indent=2), encoding="utf-8")


def _basin_geometry() -> GridGeometry:
    """Return the one shared grid geometry every basin uses (spec §8 alignment).

    All basins share the same grid family ``GRID_LABEL`` with an identical
    affine, so homogeneity (H2: same grid-label set across basins) holds and the
    COG/Zarr pair is trivially cell-for-cell aligned.
    """
    return GridGeometry(
        height=GRID_HEIGHT,
        width=GRID_WIDTH,
        res=GRID_RES,
        west=GRID_WEST,
        north=GRID_NORTH,
    )


def _time_since_epoch(times: list[dt.datetime]) -> np.ndarray:
    """Encode timestamps as CF integer days-since-epoch (spec §6.3).

    The scalar ``time`` axis is one timestamp per day; this maps each to the
    integer day count from ``_EPOCH``, the Zarr-side twin of the parquet
    ``time`` column. The mapping is exact (no truncation) for midnight daily
    stamps, so the two axes describe the *identical* instants (T2).
    """
    return np.array([(t - _EPOCH).days for t in times], dtype="int64")


def gridded_static_dir(basin_dir_path: Path) -> Path:
    """Return the ``gridded_static/`` subtree of a basin folder (spec §4)."""
    return basin_dir_path / "gridded_static"


def gridded_dynamic_dir(basin_dir_path: Path) -> Path:
    """Return the ``gridded_dynamic/`` subtree of a basin folder (spec §4)."""
    return basin_dir_path / "gridded_dynamic"


def cog_path(basin_dir_path: Path, label: str = GRID_LABEL) -> Path:
    """Return the ``gridded_static/<label>.tif`` COG path (spec §4/§8).

    ``label`` defaults to the shared :data:`GRID_LABEL`; the MS8-S2
    divergent-grid-label mutation overrides it with
    :data:`DIVERGENT_GRID_LABEL` to relocate one basin's artifact.
    """
    return gridded_static_dir(basin_dir_path) / f"{label}.tif"


def zarr_path(basin_dir_path: Path, label: str = GRID_LABEL) -> Path:
    """Return the ``gridded_dynamic/<label>.zarr`` store path (spec §4/§8).

    ``label`` defaults to the shared :data:`GRID_LABEL`; the MS8-S2
    divergent-grid-label mutation overrides it with
    :data:`DIVERGENT_GRID_LABEL` to relocate one basin's artifact.
    """
    return gridded_dynamic_dir(basin_dir_path) / f"{label}.zarr"


def write_gridded_static(
    basin_dir_path: Path, geom: GridGeometry, label: str = GRID_LABEL
) -> Path:
    """Write one basin's multiband ``gridded_static`` COG and return its path.

    Emits ``gridded_static/<label>.tif``: a tiled, overview-bearing GeoTIFF with
    the ``elevation`` field as a single band whose **description is the field
    name** (G1), standard georeferencing tags (CRS + affine, G3), units in band
    metadata (§7.3), dense ``[Y,X]`` over the bbox and delineation-neutral (§9).

    ``label`` (default :data:`GRID_LABEL`) names the artifact file; the MS8-S2
    divergent-grid-label mutation overrides it. ``geom`` likewise carries the
    grid's affine — the misaligned-shared-label mutation passes a shifted
    geometry to break G2 alignment while keeping the label shared.
    """
    log = get_logger("grids.cog")
    static_dir = gridded_static_dir(basin_dir_path)
    static_dir.mkdir(parents=True, exist_ok=True)
    path = cog_path(basin_dir_path, label)

    # Deterministic, finite elevation surface over the grid (values opaque).
    elevation = np.arange(geom.height * geom.width, dtype="float32").reshape(
        geom.height, geom.width
    )

    profile = {
        "driver": "GTiff",
        "dtype": "float32",
        "count": 1,
        "height": geom.height,
        "width": geom.width,
        "crs": RasterioCRS.from_epsg(EPSG_CODE),
        "transform": geom.transform,
        "tiled": True,
        "blockxsize": COG_BLOCK,
        "blockysize": COG_BLOCK,
        "compress": "deflate",
        "nodata": float("nan"),
    }

    with rasterio.open(path, "w", **profile) as dst:
        dst.write(elevation, 1)
        # Band description == field name (self-naming, no positional axis, G1).
        dst.set_band_description(1, GRIDDED_STATIC_FIELD)
        dst.update_tags(1, units="m")
        # Internal overviews (§8). Powers of two; nearest keeps values finite.
        dst.build_overviews([2, 4], Resampling.nearest)
        dst.update_tags(ns="rio_overview", resampling="nearest")

    log.info(
        "wrote gridded_static COG label=%s band=%s shape=%dx%d",
        label,
        GRIDDED_STATIC_FIELD,
        geom.height,
        geom.width,
    )
    return path


def _write_cf_coord(
    group: zarr.Group,
    name: str,
    values: np.ndarray,
    attrs: dict[str, object],
) -> None:
    """Write a 1-D CF coordinate array into ``group`` (helper, spec §7.3).

    The array is a single-chunk coordinate carrying its CF ``attrs`` and the
    ``_ARRAY_DIMENSIONS`` xarray/CF dimension label so consumers line the
    variable axes up with ``lat``/``lon``/``time``.
    """
    arr = group.create_array(
        name,
        shape=values.shape,
        dtype=values.dtype,
        chunks=values.shape,
        dimension_names=(name,),
    )
    arr[:] = values
    for key, val in attrs.items():
        arr.attrs[key] = val
    arr.attrs["_ARRAY_DIMENSIONS"] = [name]


def write_gridded_dynamic(
    basin_dir_path: Path,
    geom: GridGeometry,
    times: list[dt.datetime],
    label: str = GRID_LABEL,
) -> Path:
    """Write one basin's ``gridded_dynamic`` Zarr v3 store and return its path.

    Emits ``gridded_dynamic/<label>.zarr``: a Zarr v3 group with explicit CF
    ``time``/``lat``/``lon`` coordinates and a ``crs`` grid-mapping variable
    (G3), the ``era5_precipitation`` data variable and its ordinary companion
    mask ``era5_precipitation_was_filled`` as **named CF variables** (G1), all
    time-major chunked with **v3 sharding** and **blosc-zstd** compression, then
    **consolidated metadata** written over the store (§8).

    The ``time`` axis is the CF integer-since-epoch encoding of ``times`` — the
    *identical* instants as the basin's scalar ``time`` (T2 / §6.2). The first
    timestep of ``era5_precipitation`` is **NaN-filled** to exercise the §6.2
    gap convention, and the companion mask marks exactly that timestep.

    ``label`` (default :data:`GRID_LABEL`) names the artifact file; the MS8-S2
    divergent-grid-label mutation overrides it.
    """
    log = get_logger("grids.zarr")
    dynamic_dir = gridded_dynamic_dir(basin_dir_path)
    dynamic_dir.mkdir(parents=True, exist_ok=True)
    path = zarr_path(basin_dir_path, label)

    n_t = len(times)
    lat = geom.lat_centers()
    lon = geom.lon_centers()
    time_vals = _time_since_epoch(times)

    # Deterministic precipitation field [T,Y,X]; first timestep NaN-filled to
    # demonstrate the §6.2 gap convention (NaN-fill, not a missing array).
    precip = np.fromfunction(
        lambda t, y, x: (t + y * 0.1 + x * 0.01).astype("float32"),
        (n_t, geom.height, geom.width),
        dtype="float32",
    ).astype("float32")
    was_filled = np.zeros((n_t, geom.height, geom.width), dtype="int8")
    if n_t > 0:
        precip[0, :, :] = np.float32("nan")
        was_filled[0, :, :] = 1

    group = zarr.open_group(str(path), mode="w", zarr_format=3)
    # Dataset-level CF + CRS attributes (one dataset-wide CRS, spec §7.4).
    group.attrs["Conventions"] = "CF-1.8"

    _write_cf_coord(
        group,
        "time",
        time_vals,
        {
            "units": TIME_UNITS,
            "calendar": TIME_CALENDAR,
            "standard_name": "time",
            "axis": "T",
        },
    )
    _write_cf_coord(
        group,
        "lat",
        lat.astype("float64"),
        {"units": "degrees_north", "standard_name": "latitude", "axis": "Y"},
    )
    _write_cf_coord(
        group,
        "lon",
        lon.astype("float64"),
        {"units": "degrees_east", "standard_name": "longitude", "axis": "X"},
    )

    # CF grid_mapping variable carrying the single dataset-wide CRS (G3 / §7.4).
    crs_var = group.create_array("crs", shape=(), dtype="int32")
    crs_var[...] = 0
    crs_var.attrs["grid_mapping_name"] = "latitude_longitude"
    crs_var.attrs["crs_wkt"] = RasterioCRS.from_epsg(EPSG_CODE).to_wkt()
    crs_var.attrs["spatial_ref"] = f"EPSG:{EPSG_CODE}"

    chunks = (min(ZARR_TIME_CHUNK, n_t) or 1, ZARR_Y_CHUNK, ZARR_X_CHUNK)
    # One shard spans the whole [T,Y,X] cube; many time-major chunks per shard
    # (v3 sharding → sane object counts, spec §8).
    shards = (n_t or 1, geom.height, geom.width)
    compressors = [
        BloscCodec(cname="zstd", clevel=5, shuffle=BloscShuffle.shuffle)
    ]

    def _write_var(name: str, data: np.ndarray, attrs: dict[str, object]) -> None:
        var = group.create_array(
            name,
            shape=data.shape,
            dtype=data.dtype,
            chunks=chunks,
            shards=shards,
            compressors=compressors,
            dimension_names=("time", "lat", "lon"),
            fill_value=(float("nan") if data.dtype.kind == "f" else 0),
        )
        var[:] = data
        for key, val in attrs.items():
            var.attrs[key] = val
        var.attrs["_ARRAY_DIMENSIONS"] = ["time", "lat", "lon"]

    # The {source}_{variable} data field — ordinary, no special handling (§2).
    _write_var(
        GRIDDED_DYNAMIC_FIELD,
        precip,
        {"units": "mm", "grid_mapping": "crs", "standard_name": "precipitation_amount"},
    )
    # The {field}_was_filled companion mask — an ordinary variable, no magic.
    _write_var(
        COMPANION_MASK_FIELD,
        was_filled,
        {"units": "1", "grid_mapping": "crs", "long_name": "was-filled mask"},
    )

    # --- MED-5 WRITER/READER HAND-OFF (spec §8; planning/MS2/steps.md) ---------
    # Consolidated metadata is a WRITER-side property. zarr-python writes the
    # store's consolidated metadata into the root `zarr.json` so a single GET
    # learns the whole store. MS4 MUST confirm the Rust `zarrs` reader reads via
    # this §8 consolidated path (or classify it an R3 byte-deep skip with a
    # reason). A writer/reader mismatch is fixed by REGENERATING the fixture,
    # never a reader workaround. See conformance/README.md (Rule 3).
    # ---------------------------------------------------------------------------
    zarr.consolidate_metadata(str(path))
    # Sort the consolidated-metadata members so the committed root zarr.json is
    # byte-deterministic across regenerates (see _stabilize_consolidated_metadata).
    _stabilize_consolidated_metadata(path)

    log.info(
        "wrote gridded_dynamic Zarr label=%s vars=%s T=%d shape=%dx%d sharded",
        label,
        [GRIDDED_DYNAMIC_FIELD, COMPANION_MASK_FIELD],
        n_t,
        geom.height,
        geom.width,
    )
    return path


def write_gridded_for_basin(dataset_root: Path, basin: BasinSpec) -> tuple[Path, Path]:
    """Write the COG + Zarr for one basin (shared label, aligned) and return both.

    Both artifacts use the single shared :data:`GRID_LABEL` and the same
    :class:`GridGeometry`, so they are cell-for-cell aligned (G2). The Zarr time
    axis is the basin's scalar ``time`` axis (T2).
    """
    from hdx_fixtures.scalar import basin_dir

    basin_dir_path = basin_dir(dataset_root, basin.basin_id)
    geom = _basin_geometry()
    times = _time_axis(basin)
    cog = write_gridded_static(basin_dir_path, geom)
    store = write_gridded_dynamic(basin_dir_path, geom, times)
    return cog, store


def write_grids(dataset_root: Path) -> list[Path]:
    """Write the COG + Zarr for every basin; return all emitted artifact paths."""
    written: list[Path] = []
    for basin in BASINS:
        cog, store = write_gridded_for_basin(dataset_root, basin)
        written.extend((cog, store))
    return written


# --- multi_grid_multi_static writers (merge-gen M1) ---------------------------
#
# These emit a basin's TWO static COGs (dem, landcover) and TWO dynamic Zarrs
# (era5, merit), each with its OWN distinctly-named field. They mirror the
# baseline writers above but take the band/data-var field name as an argument
# (the baseline writers hardcode the single-family field names, so they are left
# untouched). All four artifacts use the shared baseline geometry/time axis.


def write_gridded_static_field(
    basin_dir_path: Path, geom: GridGeometry, label: str, field_name: str
) -> Path:
    """Write a single-band ``gridded_static`` COG whose band is ``field_name``.

    Identical to :func:`write_gridded_static` except the band description (the
    self-naming G1 field name) is ``field_name`` rather than the baseline
    :data:`GRIDDED_STATIC_FIELD`, so a multi-family tree carries a distinct band
    field per static ``label``. The raster is f32, tiled, overview-bearing and
    georeferenced over ``geom`` (G3), dense ``[Y,X]`` and delineation-neutral (§9).
    """
    log = get_logger("grids.cog")
    static_dir = gridded_static_dir(basin_dir_path)
    static_dir.mkdir(parents=True, exist_ok=True)
    path = cog_path(basin_dir_path, label)

    surface = np.arange(geom.height * geom.width, dtype="float32").reshape(
        geom.height, geom.width
    )

    profile = {
        "driver": "GTiff",
        "dtype": "float32",
        "count": 1,
        "height": geom.height,
        "width": geom.width,
        "crs": RasterioCRS.from_epsg(EPSG_CODE),
        "transform": geom.transform,
        "tiled": True,
        "blockxsize": COG_BLOCK,
        "blockysize": COG_BLOCK,
        "compress": "deflate",
        "nodata": float("nan"),
    }

    with rasterio.open(path, "w", **profile) as dst:
        dst.write(surface, 1)
        # Band description == field name (self-naming, no positional axis, G1).
        dst.set_band_description(1, field_name)
        dst.update_tags(1, units="m")
        dst.build_overviews([2, 4], Resampling.nearest)
        dst.update_tags(ns="rio_overview", resampling="nearest")

    log.info(
        "wrote multi-family gridded_static COG label=%s band=%s shape=%dx%d",
        label,
        field_name,
        geom.height,
        geom.width,
    )
    return path


def write_gridded_dynamic_field(
    basin_dir_path: Path,
    geom: GridGeometry,
    times: list[dt.datetime],
    label: str,
    field_name: str,
) -> Path:
    """Write a ``gridded_dynamic`` Zarr v3 store with data-var ``field_name``.

    Identical to :func:`write_gridded_dynamic` except the single data variable is
    named ``field_name`` (with its ordinary ``{field_name}_was_filled`` companion
    mask) rather than the baseline :data:`GRIDDED_DYNAMIC_FIELD`, so a multi-family
    tree carries distinct data-var fields per dynamic ``label``. The ``time`` axis
    is the basin's scalar ``time`` axis (T2), the first timestep is NaN-filled
    (§6.2) and the companion mask marks it. Sharded + consolidated metadata (§8).
    """
    log = get_logger("grids.zarr")
    dynamic_dir = gridded_dynamic_dir(basin_dir_path)
    dynamic_dir.mkdir(parents=True, exist_ok=True)
    path = zarr_path(basin_dir_path, label)
    companion = f"{field_name}_was_filled"

    n_t = len(times)
    lat = geom.lat_centers()
    lon = geom.lon_centers()
    time_vals = _time_since_epoch(times)

    data = np.fromfunction(
        lambda t, y, x: (t + y * 0.1 + x * 0.01).astype("float32"),
        (n_t, geom.height, geom.width),
        dtype="float32",
    ).astype("float32")
    was_filled = np.zeros((n_t, geom.height, geom.width), dtype="int8")
    if n_t > 0:
        data[0, :, :] = np.float32("nan")
        was_filled[0, :, :] = 1

    group = zarr.open_group(str(path), mode="w", zarr_format=3)
    group.attrs["Conventions"] = "CF-1.8"

    _write_cf_coord(
        group,
        "time",
        time_vals,
        {
            "units": TIME_UNITS,
            "calendar": TIME_CALENDAR,
            "standard_name": "time",
            "axis": "T",
        },
    )
    _write_cf_coord(
        group,
        "lat",
        lat.astype("float64"),
        {"units": "degrees_north", "standard_name": "latitude", "axis": "Y"},
    )
    _write_cf_coord(
        group,
        "lon",
        lon.astype("float64"),
        {"units": "degrees_east", "standard_name": "longitude", "axis": "X"},
    )

    crs_var = group.create_array("crs", shape=(), dtype="int32")
    crs_var[...] = 0
    crs_var.attrs["grid_mapping_name"] = "latitude_longitude"
    crs_var.attrs["crs_wkt"] = RasterioCRS.from_epsg(EPSG_CODE).to_wkt()
    crs_var.attrs["spatial_ref"] = f"EPSG:{EPSG_CODE}"

    chunks = (min(ZARR_TIME_CHUNK, n_t) or 1, ZARR_Y_CHUNK, ZARR_X_CHUNK)
    shards = (n_t or 1, geom.height, geom.width)
    compressors = [
        BloscCodec(cname="zstd", clevel=5, shuffle=BloscShuffle.shuffle)
    ]

    def _write_var(name: str, arr_data: np.ndarray, attrs: dict[str, object]) -> None:
        var = group.create_array(
            name,
            shape=arr_data.shape,
            dtype=arr_data.dtype,
            chunks=chunks,
            shards=shards,
            compressors=compressors,
            dimension_names=("time", "lat", "lon"),
            fill_value=(float("nan") if arr_data.dtype.kind == "f" else 0),
        )
        var[:] = arr_data
        for key, val in attrs.items():
            var.attrs[key] = val
        var.attrs["_ARRAY_DIMENSIONS"] = ["time", "lat", "lon"]

    _write_var(
        field_name,
        data,
        {"units": "mm", "grid_mapping": "crs", "standard_name": "precipitation_amount"},
    )
    _write_var(
        companion,
        was_filled,
        {"units": "1", "grid_mapping": "crs", "long_name": "was-filled mask"},
    )

    zarr.consolidate_metadata(str(path))
    _stabilize_consolidated_metadata(path)

    log.info(
        "wrote multi-family gridded_dynamic Zarr label=%s vars=%s T=%d shape=%dx%d sharded",
        label,
        [field_name, companion],
        n_t,
        geom.height,
        geom.width,
    )
    return path


def write_multi_family_grids_for_basin(
    dataset_root: Path, basin: BasinSpec
) -> list[Path]:
    """Write one basin's TWO static COGs + TWO dynamic Zarrs (multi-family).

    Emits the static labels of :data:`SECOND_STATIC_LABELS` (dem, landcover) and
    the dynamic labels of :data:`SECOND_DYNAMIC_LABELS` (era5, merit), each with
    its own field, all over the shared baseline :class:`GridGeometry` and the
    basin's scalar ``time`` axis (T2). Returns every emitted artifact path.
    """
    from hdx_fixtures.scalar import basin_dir

    basin_dir_path = basin_dir(dataset_root, basin.basin_id)
    geom = _basin_geometry()
    times = _time_axis(basin)

    written: list[Path] = []
    for label, field_name in SECOND_STATIC_LABELS:
        written.append(
            write_gridded_static_field(basin_dir_path, geom, label, field_name)
        )
    for label, field_name in SECOND_DYNAMIC_LABELS:
        written.append(
            write_gridded_dynamic_field(
                basin_dir_path, geom, times, label, field_name
            )
        )
    return written


def write_multi_family_grids(dataset_root: Path) -> list[Path]:
    """Write the two-family COG/Zarr set for every basin; return all paths."""
    written: list[Path] = []
    for basin in BASINS:
        written.extend(write_multi_family_grids_for_basin(dataset_root, basin))
    return written


# --- MS8-S2 re-emit seam: one basin's grids under a different label/geometry ---
#
# These helpers let a single mutation (mutate.py) re-emit ONE basin's gridded
# artifacts under a divergent LABEL and/or a MISALIGNED GEOMETRY, reusing the
# baseline writers above so the in-file artifact naming/serialization stays
# consistent (never a raw dir rename). They never touch the baseline writers' own
# behaviour: the default-argument writers above are unchanged for the baseline.


def misaligned_geometry() -> GridGeometry:
    """Return a geometry half-cell-shifted east of the baseline (G2 negative).

    Identical to :func:`_basin_geometry` except ``west`` is
    :data:`MISALIGNED_GRID_WEST` (a half-cell shift). Re-emitting ONLY a basin's
    COG at this geometry — its Zarr left at the baseline — keeps the shared
    ``era5`` label present in both subtrees (H2 stays pass) while their extents no
    longer coincide cell-for-cell, so check_g2 ran:fails for that basin. The
    width/res/height are unchanged so the COG stays a georeferenced raster (G3
    stays pass) and the label is still ``era5`` (H2 stays pass).
    """
    return GridGeometry(
        height=GRID_HEIGHT,
        width=GRID_WIDTH,
        res=GRID_RES,
        west=MISALIGNED_GRID_WEST,
        north=GRID_NORTH,
    )


def reemit_basin_grids_under_label(
    dataset_root: Path, basin: BasinSpec, label: str
) -> tuple[Path, Path]:
    """Re-emit a basin's COG **and** Zarr under ``label`` (divergent-label seam).

    Removes the basin's baseline ``gridded_static``/``gridded_dynamic`` artifacts
    (so only the relabelled pair remains) and re-writes both under ``label`` at
    the **baseline** geometry/time axis. Because BOTH subtrees move to ``label``,
    the basin's shared-label COG+Zarr still coincide (G2 stays pass) while its
    grid-label SET becomes ``{label}`` — the H2 cross-basin negative.
    """
    from hdx_fixtures.scalar import basin_dir

    basin_dir_path = basin_dir(dataset_root, basin.basin_id)
    geom = _basin_geometry()
    times = _time_axis(basin)

    # Drop the baseline-labelled artifacts so the basin carries ONLY the relabelled
    # pair (the divergent label set is {label}, not {era5, label}).
    cog_path(basin_dir_path).unlink()
    _rmtree(zarr_path(basin_dir_path))

    cog = write_gridded_static(basin_dir_path, geom, label)
    store = write_gridded_dynamic(basin_dir_path, geom, times, label)
    return cog, store


def reemit_basin_zarr_with_times(
    dataset_root: Path, basin: BasinSpec, times: list[dt.datetime]
) -> Path:
    """Re-emit ONLY a basin's Zarr (same :data:`GRID_LABEL`) with a given ``time`` axis.

    Removes the basin's baseline ``gridded_dynamic`` Zarr and re-writes it under the
    shared :data:`GRID_LABEL` at the **baseline** geometry but with ``times`` as its
    ``time`` coordinate (CF integer days-since-epoch). Used by the MS8-S3
    still-conformant irregular-time-axis mutation to keep the Zarr ``time`` axis
    IDENTICAL to the basin's rewritten scalar ``time`` column (T2 preserved). The
    COG, the grid geometry, and every other artifact are untouched — only the one
    basin's Zarr ``time`` coordinate (and the chunked arrays sized to ``len(times)``)
    differ, matching the scalar rewrite cell-for-cell.
    """
    from hdx_fixtures.scalar import basin_dir

    basin_dir_path = basin_dir(dataset_root, basin.basin_id)
    geom = _basin_geometry()
    _rmtree(zarr_path(basin_dir_path))
    return write_gridded_dynamic(basin_dir_path, geom, times, GRID_LABEL)


def reemit_basin_cog_with_geometry(
    dataset_root: Path, basin: BasinSpec, geom: GridGeometry
) -> Path:
    """Re-emit ONLY a basin's COG (same :data:`GRID_LABEL`) at ``geom`` (G2 seam).

    Removes the basin's baseline COG and re-writes it under the shared
    :data:`GRID_LABEL` at ``geom`` (a misaligned geometry), leaving the basin's
    Zarr at the baseline geometry. The shared ``era5`` label now appears in both
    subtrees but their extents diverge, so check_g2 ran:fails for that basin while
    H2 (label set still ``{era5}``) and G3 (georef intact) stay pass.
    """
    from hdx_fixtures.scalar import basin_dir

    basin_dir_path = basin_dir(dataset_root, basin.basin_id)
    cog_path(basin_dir_path).unlink()
    return write_gridded_static(basin_dir_path, geom, GRID_LABEL)


def _rmtree(path: Path) -> None:
    """Remove a directory tree (a Zarr store is a directory of many files)."""
    import shutil

    shutil.rmtree(path)
