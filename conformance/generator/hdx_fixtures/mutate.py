"""Derive the two minimal invalid fixtures from the valid baseline (MS2-S4).

LOW-2 (hard rule, see ``conformance/README.md`` Rule 2): every invalid fixture
is produced **programmatically from the single valid baseline via exactly one
surgical mutation each**. Fixture trees are NEVER hand-edited. This module is the
derivation layer: it copies the complete valid baseline tree, then applies one —
and only one — targeted mutation, so each invalid is exactly one mutation off a
known-good baseline.

Two invalids are derived, each pinning exactly one spec check (spec §14):

* ``invalid/wrong-format-version/`` — copy the baseline, then overwrite
  ``manifest.json`` so ``format_version: "0.2"`` (everything else byte-identical
  to the baseline manifest). Pins **M2** (the §0 hard version cut).
* ``invalid/missing-root-rollup/`` — copy the baseline, then **delete** one root
  rollup (``outlines.geoparquet``). Pins **L1** (root rollups exist, §4).

After each derivation the invalid-side self-assertion
(:func:`hdx_fixtures.assertions.assert_differs_in_exactly_one_way`) confirms the
tree differs from the baseline in **exactly the one intended way**, enforcing the
one-mutation invariant at generation time.

This module emits no contract logic and is dev-only — like the rest of the
harness it merely emits bytes a reader will later read (spec §10 / architecture
§7 R2).
"""

import json
import shutil
from enum import Enum
from pathlib import Path

from hdx_fixtures import get_logger
from hdx_fixtures.manifest import MANIFEST_FIELDS

# The mutated format_version for the wrong-format-version invalid. Any value other
# than the baseline "0.1" is rejected outright by M2 (the §0 hard cut); "0.2" is a
# plausible future version that the v0.1 reader MUST reject.
WRONG_FORMAT_VERSION: str = "0.2"

# The single root rollup deleted by the missing-root-rollup invalid. L1 requires
# BOTH scalar_static.parquet AND outlines.geoparquet at the root (spec §4); this
# removes exactly one of them. Documented in the README check-id table.
MISSING_ROOT_ROLLUP: str = "outlines.geoparquet"


class Invalid(Enum):
    """The two minimal invalid fixtures, each pinning one spec check (§14).

    Using an enum (not a bare string) keeps the invalid identity a closed domain
    state: a derivation routine cannot be called for an unknown invalid, and the
    folder name + pinned check id travel with the variant.
    """

    WRONG_FORMAT_VERSION = "wrong-format-version"
    MISSING_ROOT_ROLLUP = "missing-root-rollup"

    @property
    def pinned_check(self) -> str:
        """Return the single spec §14 check this invalid pins (M2 or L1)."""
        if self is Invalid.WRONG_FORMAT_VERSION:
            return "M2"
        return "L1"


def invalid_root(repo_root: Path, invalid: Invalid) -> Path:
    """Return the ``conformance/invalid/<name>/`` tree root for ``invalid``."""
    return repo_root / "conformance" / "invalid" / invalid.value


def _copy_baseline(baseline_root: Path, target_root: Path) -> None:
    """Copy the complete valid baseline tree to ``target_root`` (replacing it).

    The copy is byte-for-byte (``copy2`` preserves file content; metadata is
    irrelevant to the committed fixture). ``target_root`` is removed first so the
    derivation is deterministic and re-runnable — re-deriving never leaves stale
    files from a previous run.
    """
    if target_root.exists():
        shutil.rmtree(target_root)
    shutil.copytree(baseline_root, target_root, copy_function=shutil.copy2)


def _mutate_format_version(target_root: Path) -> None:
    """Overwrite ``manifest.json`` so ``format_version`` is the wrong version (M2).

    The manifest is re-read, the single ``format_version`` value is replaced with
    :data:`WRONG_FORMAT_VERSION`, and the file is rewritten with the **identical**
    serialization the baseline writer used (2-space indent, trailing newline, key
    order preserved) so the tree differs from the baseline in *exactly* this one
    value and nothing else.
    """
    manifest_path = target_root / "manifest.json"
    obj = json.loads(manifest_path.read_text(encoding="utf-8"))
    # Preserve the six-field floor + key order; mutate only the one value (M2).
    obj["format_version"] = WRONG_FORMAT_VERSION
    ordered = {field: obj[field] for field in MANIFEST_FIELDS}
    manifest_path.write_text(json.dumps(ordered, indent=2) + "\n", encoding="utf-8")


def _mutate_missing_root_rollup(target_root: Path) -> None:
    """Delete exactly one root rollup so the dataset violates L1.

    Removes :data:`MISSING_ROOT_ROLLUP` (``outlines.geoparquet``) from the dataset
    root. L1 (spec §4) requires both root rollups; deleting one — and only one —
    is the single surgical mutation. The rest of the tree stays byte-identical to
    the baseline.
    """
    rollup = target_root / MISSING_ROOT_ROLLUP
    rollup.unlink()


def derive_invalid(baseline_root: Path, repo_root: Path, invalid: Invalid) -> Path:
    """Derive one invalid tree from the baseline via exactly one mutation.

    Copies the complete valid baseline, then dispatches the single mutation for
    ``invalid`` (LOW-2). Returns the derived tree root. The caller runs the
    "differs in exactly one way" self-assertion afterwards.
    """
    log = get_logger("mutate")
    target_root = invalid_root(repo_root, invalid)
    _copy_baseline(baseline_root, target_root)

    if invalid is Invalid.WRONG_FORMAT_VERSION:
        _mutate_format_version(target_root)
    else:
        _mutate_missing_root_rollup(target_root)

    log.info(
        "derived invalid=%s pins=%s root=%s",
        invalid.value,
        invalid.pinned_check,
        target_root,
    )
    return target_root
