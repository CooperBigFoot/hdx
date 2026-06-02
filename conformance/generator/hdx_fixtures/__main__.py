"""S1 harness entry point: smoke-import the pinned deps, print one status line.

Run as ``python -m hdx_fixtures`` (regenerate.sh does this inside the venv).

For MS2-S1 this is a STUB: it proves the pinned dependency closure imports on the
declared interpreter (so the pins resolve), logs each import at debug level, then
prints the single user-facing status line and exits 0. No fixtures are emitted
yet — later MS2 steps add the actual generation.
"""

import importlib
import sys

from hdx_fixtures import configure_logging, get_logger

# The pinned dependency closure later MS2 steps build on. Import names differ
# from distribution names for a couple of these (osgeo-free rasterio imports as
# ``rasterio``; pyarrow as ``pyarrow``); the value is the human label.
_PINNED_IMPORTS: tuple[tuple[str, str], ...] = (
    ("numpy", "numpy"),
    ("pandas", "pandas"),
    ("pyarrow", "pyarrow (parquet + geoparquet)"),
    ("pyarrow.parquet", "pyarrow.parquet"),
    ("xarray", "xarray"),
    ("zarr", "zarr (v3)"),
    ("rasterio", "rasterio (COG)"),
    ("rioxarray", "rioxarray"),
    ("geopandas", "geopandas"),
    ("shapely", "shapely"),
)


def smoke_import() -> None:
    """Import every pinned dependency, logging each; raise on the first failure.

    A successful run proves the exact-version pins resolve on the declared
    interpreter — the reproducibility check for the harness.
    """
    log = get_logger("smoke")
    for module_name, label in _PINNED_IMPORTS:
        module = importlib.import_module(module_name)
        version = getattr(module, "__version__", "n/a")
        log.debug("imported %s (%s) version=%s", module_name, label, version)
    log.info("smoke import OK: all %d pinned deps imported", len(_PINNED_IMPORTS))


def main() -> int:
    configure_logging()
    smoke_import()
    # User-facing status line (output, not a diagnostic) — see architecture §2.
    print("MS2-S1: harness only, no fixtures yet")
    return 0


if __name__ == "__main__":
    sys.exit(main())
