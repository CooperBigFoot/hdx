"""Emit ``manifest.json`` — the irreducible six-field floor (spec §11).

The manifest declares **only what is not derivable** from the bytes: the six
floor fields of spec §11. No seventh / derivable key is ever written (inert /
agnostic discipline, spec §1/§11): no content hash, no data-version, no field
catalog, no basin list, no transform / role / semantic / provenance key.

The emitted JSON shape matches MS1's ``schemas/manifest.schema.json`` (a
shape-only dependency on MS1 — this module links nothing from ``hdx-core``).
``format_version`` is the hard version cut and is written as the literal
``"0.1"`` (spec §0/§14 M1/M2).
"""

import datetime as dt
import json
from pathlib import Path

from hdx_fixtures import get_logger

# The six floor fields, in spec §11 order. This tuple is the single source of
# truth for "exactly six fields, no more, no less" — the writer self-assertion
# (assertions.assert_manifest_floor) re-derives the field set from it.
MANIFEST_FIELDS: tuple[str, ...] = (
    "format_version",
    "name",
    "created_at",
    "producer_version",
    "crs",
    "cadence",
)

# Hard version cut: the valid baseline declares "0.1" (spec §0/§14 M2). The
# wrong-format-version invalid (MS2-S4) mutates only this value.
FORMAT_VERSION: str = "0.1"

# Dataset-wide CRS (spec §7.4 / §11). EPSG:4326 is the recommended CRS and the
# one the gridded half (MS2-S3) carries in its files; M5 cross-checks them.
CRS: str = "EPSG:4326"

# Dataset-wide cadence/calendar convention (spec §6.4 / §11). The realized scalar
# `time` axes (one timestamp per day) are consistent with "daily" (M6).
CADENCE: str = "daily"

# A fixed, deterministic created_at so regeneration is byte-reproducible. RFC 3339
# with the `Z` zulu form (spec §11 / §14 M4).
CREATED_AT: str = "2026-06-01T00:00:00Z"

# Generic dataset identity (spec §11) — not a role label.
NAME: str = "hdx-conformance-valid-minimal"

# The tool/version that wrote the dataset (spec §11).
PRODUCER_VERSION: str = "hdx-fixtures 0.1.0"


def build_manifest() -> dict[str, str]:
    """Return the six-field manifest object (spec §11) in floor order.

    The returned dict has **exactly** the six keys of :data:`MANIFEST_FIELDS`.
    ``created_at`` is validated to be RFC-3339-parseable here, at the boundary,
    so an ill-formed constant cannot reach disk.
    """
    # Parse-don't-validate at the boundary: prove created_at is RFC 3339 before
    # it is ever written. The `Z` form is accepted by fromisoformat on 3.12.
    dt.datetime.fromisoformat(CREATED_AT.replace("Z", "+00:00"))

    return {
        "format_version": FORMAT_VERSION,
        "name": NAME,
        "created_at": CREATED_AT,
        "producer_version": PRODUCER_VERSION,
        "crs": CRS,
        "cadence": CADENCE,
    }


def write_manifest(dataset_root: Path) -> Path:
    """Write ``manifest.json`` into ``dataset_root`` and return its path.

    The file is written with a trailing newline and 2-space indentation so the
    committed fixture is stable and human-diffable.
    """
    log = get_logger("manifest")
    dataset_root.mkdir(parents=True, exist_ok=True)
    manifest_path = dataset_root / "manifest.json"

    manifest = build_manifest()
    manifest_path.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")

    log.info(
        "wrote manifest.json fields=%d format_version=%s",
        len(manifest),
        manifest["format_version"],
    )
    return manifest_path
