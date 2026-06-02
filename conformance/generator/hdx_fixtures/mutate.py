"""Derive the minimal invalid fixtures from the valid baseline (MS2-S4 / MS8-S2).

LOW-2 (hard rule, see ``conformance/README.md`` Rule 2): every invalid fixture
is produced **programmatically from the single valid baseline via exactly one
surgical mutation each**. Fixture trees are NEVER hand-edited. This module is the
derivation layer: it copies the complete valid baseline tree, then applies one —
and only one — targeted mutation, so each invalid is exactly one mutation off a
known-good baseline.

The invalids are derived, each pinning exactly one spec check (spec §14):

* ``invalid/wrong-format-version/`` — copy the baseline, then overwrite
  ``manifest.json`` so ``format_version: "0.2"`` (everything else byte-identical
  to the baseline manifest). Pins **M2** (the §0 hard version cut).
* ``invalid/extra-manifest-field/`` — copy the baseline, then add one derivable
  key (``content_hash``) to ``manifest.json`` (a 7th field). Pins **M3** (the
  six-field floor; spec §0/§11). A manifest-only rewrite (one file differs).
* ``invalid/empty-cadence/`` — copy the baseline, then set ``cadence: ""`` in
  ``manifest.json``. Pins **M4** (``crs``/``cadence`` non-empty; spec §6.4). A
  manifest-only rewrite (one file differs).
* ``invalid/missing-root-rollup/`` — copy the baseline, then **delete** one root
  rollup (``outlines.geoparquet``). Pins **L1** (root rollups exist, §4).

The **Bucket-B** parquet/layout negatives (MS8-S3) each copy the baseline, then
apply one surgical mutation that yields a clean ``conformant:false`` report with
exactly one §14 check ``ran:fail`` (every other check passes or honestly skips):

* ``invalid/missing-basin-id-column/`` — drop ``basin_id`` from one basin's
  ``scalar_dynamic.parquet``. Pins **I1** (``basin_id`` present, §3).
* ``invalid/basin-id-folder-mismatch/`` — rewrite one basin's in-file
  ``basin_id`` to a unique foreign value. Pins **I2** (folder agreement, §3).
* ``invalid/ragged-field-schema/`` — rename one basin's ``scalar_dynamic`` data
  field ``streamflow`` → ``flow``. Pins **H1** (homogeneous schema, §5).
* ``invalid/non-monotonic-time/`` — write one basin's ``scalar_dynamic`` ``time``
  descending. Pins **T1** (``time`` sorted, §6.3).
* ``invalid/missing-gridded-dynamic-subtree/`` — delete one basin's
  ``gridded_dynamic/`` subtree. Pins **L2** (per-basin artifacts, §4).

**I3 (duplicate basin_id) is NOT in this set** — an I3-only on-disk negative is not
representable in v0.1 (see the I3 finding below): ``check_i3`` reads
the per-basin ``scalar_dynamic`` in-file ids, the ``scalar_static`` rollup values are
never read, and any per-basin duplicate co-trips I2. It is a Bucket-C finding, not a
Bucket-B fixture.

The ``extra-manifest-field`` (M3) and ``empty-cadence`` (M4) invalids are
**Bucket-A entry-gate negatives**: ``validate`` rejects them in
``Manifest::from_json`` *before* discovery, so each is an
``Err(ValidateError::Manifest(..))`` — never a ``conformant:false`` report (the
report-vs-error split, spec §0/§10/§14). M2 (``wrong-format-version``) already
existed from MS2; M3/M4 join it here.

**M6-unreachable note (empty-cadence).** ``check_m6`` rule (a) would ``ran:fail``
on an empty cadence, but the M4 entry gate rejects an empty cadence *first*, so
``validate(empty-cadence)`` returns ``Err(Manifest(EmptyCadence))`` before
``build_report`` ever runs rule (a). The empty-cadence fixture therefore pins
**M4**, not M6 — rule (a)'s fail leg is dead code on the validate path.

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

import pyarrow as pa
import pyarrow.parquet as pq

from hdx_fixtures import get_logger
from hdx_fixtures.grids import GRID_LABEL, gridded_dynamic_dir
from hdx_fixtures.manifest import MANIFEST_FIELDS
from hdx_fixtures.scalar import BASINS, DYNAMIC_FIELD, basin_dir

# The mutated format_version for the wrong-format-version invalid. Any value other
# than the baseline "0.1" is rejected outright by M2 (the §0 hard cut); "0.2" is a
# plausible future version that the v0.1 reader MUST reject.
WRONG_FORMAT_VERSION: str = "0.2"

# The single root rollup deleted by the missing-root-rollup invalid. L1 requires
# BOTH scalar_static.parquet AND outlines.geoparquet at the root (spec §4); this
# removes exactly one of them. Documented in the README check-id table.
MISSING_ROOT_ROLLUP: str = "outlines.geoparquet"

# The one derivable key the extra-manifest-field invalid adds to make a 7th field
# (M3). A content hash is the canonical inert/agnostic violation: it is derivable
# from the bytes, so spec §0/§11 forbid it in the six-field floor. The value is
# arbitrary — M3 rejects the *presence* of any extra key, regardless of value.
EXTRA_MANIFEST_FIELD: str = "content_hash"
EXTRA_MANIFEST_FIELD_VALUE: str = "deadbeef"

# The mutated cadence value for the empty-cadence invalid (M4). M4 requires
# ``crs``/``cadence`` to be non-empty strings (spec §6.4 / §11); an empty cadence
# is rejected at the entry gate. (See the M6-unreachable note in the module
# docstring: M4 rejects this BEFORE check_m6 rule (a) could fire.)
EMPTY_CADENCE: str = ""

# --- Bucket-B scalar/identity/homogeneity/time/layout mutation targets (MS8-S3) -

# The single basin every Bucket-B parquet/layout mutation surgically targets. The
# LAST basin (``0003``) is chosen so the mutation is one basin off the otherwise
# byte-identical baseline; the other two basins stay conformant, which is what
# makes each negative a CLEAN one-check ``ran:fail`` (the rule fires on the one
# divergent basin and every other check still passes/skips).
_MUTATED_BASIN: str = BASINS[-1].basin_id  # "0003"

# I2 (basin-id/folder mismatch): the value the mutated basin's in-file ``basin_id``
# is rewritten to. It MUST (a) differ from the basin's ``basin=<id>`` folder so I2
# fires, and (b) stay UNIQUE across the dataset so I3 still passes (the I2 negative
# is folder-disagreement ONLY, not a duplicate). ``9999`` is no basin's folder.
I2_FOREIGN_BASIN_ID: str = "9999"

# I3 (duplicate basin_id): NO clean on-disk negative exists in v0.1 (a code-grounded
# finding, NOT in this step's committed set). ``check_i3`` reads its input from the
# per-basin ``scalar_dynamic`` in-file ``basin_id`` values the discovery model
# surfaces (``validate.rs`` ``in_file_basin_ids`` → ``BasinScalar::basin_id_in_file``);
# the ``scalar_static`` ROLLUP basin_id VALUES are never read into the model (only the
# rollup's column-PRESENCE flag is, for I1's static leg). Two consequences, both
# verified empirically against ``validate``:
#   * the ``scalar_static`` rollup-row duplicate (steps.md's pinned form) produces a
#     ``conformant:TRUE`` dataset — I3 never fires (the rollup values are not read);
#   * the per-basin ``scalar_dynamic`` duplicate fires I3 but ALSO trips I2 (the
#     duplicated in-file id ≠ its ``basin=<id>`` folder) — two checks fail, not one.
# I2 and I3 are coupled by the model (both consume the same per-basin first-distinct
# in-file id; a duplicate among distinct folders implies a folder disagreement), so an
# I3-ONLY ``conformant:false`` negative is NOT REPRESENTABLE on disk in v0.1. This is a
# Bucket-C finding (like G1/G3/Geo1), not the Bucket-B fixture S3 specified. No
# ``duplicate-basin-id`` tree is committed by this attempt.

# H1 (ragged field schema): the divergent dynamic-field name one basin's
# ``scalar_dynamic`` is renamed to. The dtype/quadrant/nullability are unchanged —
# ONLY the field NAME diverges (``streamflow`` → ``flow``), so the per-basin schema
# key (name, quadrant, dtype, grid_label) differs for exactly the one basin and H1
# fires while T1/I1/I2/I3 stay pass (basin_id is never catalogued as a field).
H1_DIVERGENT_FIELD: str = "flow"

# L2 (missing per-basin gridded_dynamic subtree): the per-basin subtree directory
# deleted from ONE basin. ``declares_gridded_dynamic`` stays true dataset-wide (the
# other basins keep their Zarr), so the rule requires this artifact for EVERY basin
# and the one missing subtree ⇒ ``ran:fail``. H2 does NOT co-fail: the surviving COG
# keeps the ``era5`` static label, so every basin's label set is ``{era5}``.
#
# REJECTED ALTERNATIVE (noted, not committed): leaving an EMPTY ``gridded_dynamic/``
# directory rather than deleting the whole subtree — an empty dir is not a committable
# git artifact and the layout walk would treat it identically, so deleting the subtree
# is the committed form (the H2-collision caveat in steps.md §0 Bucket-B / L2).


class Invalid(Enum):
    """The minimal invalid fixtures, each pinning one spec check (§14).

    Using an enum (not a bare string) keeps the invalid identity a closed domain
    state: a derivation routine cannot be called for an unknown invalid, and the
    folder name + pinned check id travel with the variant.
    """

    WRONG_FORMAT_VERSION = "wrong-format-version"
    EXTRA_MANIFEST_FIELD = "extra-manifest-field"
    EMPTY_CADENCE = "empty-cadence"
    MISSING_ROOT_ROLLUP = "missing-root-rollup"
    # Bucket-B scalar/identity/homogeneity/time/layout negatives (MS8-S3). Each is
    # a clean ``conformant:false`` report with exactly one §14 check ``ran:fail``.
    # I3 (duplicate-basin-id) is intentionally ABSENT: an I3-only on-disk negative is
    # not representable in v0.1 (the I3 finding above).
    MISSING_BASIN_ID_COLUMN = "missing-basin-id-column"
    BASIN_ID_FOLDER_MISMATCH = "basin-id-folder-mismatch"
    RAGGED_FIELD_SCHEMA = "ragged-field-schema"
    NON_MONOTONIC_TIME = "non-monotonic-time"
    MISSING_GRIDDED_DYNAMIC_SUBTREE = "missing-gridded-dynamic-subtree"

    @property
    def pinned_check(self) -> str:
        """Return the single spec §14 check this invalid pins.

        Bucket-A entry-gate ``Err`` negatives: M2/M3/M4/L1. Bucket-B
        ``conformant:false`` negatives: I1/I2/I3/H1/T1/L2 (MS8-S3).
        """
        if self is Invalid.WRONG_FORMAT_VERSION:
            return "M2"
        if self is Invalid.EXTRA_MANIFEST_FIELD:
            return "M3"
        if self is Invalid.EMPTY_CADENCE:
            return "M4"
        if self is Invalid.MISSING_ROOT_ROLLUP:
            return "L1"
        if self is Invalid.MISSING_BASIN_ID_COLUMN:
            return "I1"
        if self is Invalid.BASIN_ID_FOLDER_MISMATCH:
            return "I2"
        if self is Invalid.RAGGED_FIELD_SCHEMA:
            return "H1"
        if self is Invalid.NON_MONOTONIC_TIME:
            return "T1"
        return "L2"


def invalid_root(repo_root: Path, invalid: Invalid) -> Path:
    """Return the ``conformance/invalid/<name>/`` tree root for ``invalid``."""
    return repo_root / "conformance" / "invalid" / invalid.value


def _copy_baseline(baseline_root: Path, target_root: Path) -> None:
    """Copy the complete valid baseline tree to ``target_root`` (replacing it).

    The copy is byte-for-byte (``copy2`` preserves file content; metadata is
    irrelevant to the committed fixture). ``target_root`` is removed first so the
    derivation is deterministic and re-runnable — re-deriving never leaves stale
    files from a previous run.

    The baseline's golden artifacts (``*.golden.json`` — the committed
    ``describe``/``validate`` outputs of the *valid* baseline) are **excluded**:
    they describe the baseline, not the mutated invalid, so copying them would
    plant a stale, meaningless golden in the invalid tree. A per-fixture golden,
    when a later step needs one, is regenerated from the Rust verb against the
    mutated tree — never inherited from the baseline copy.
    """
    if target_root.exists():
        shutil.rmtree(target_root)
    shutil.copytree(
        baseline_root,
        target_root,
        copy_function=shutil.copy2,
        ignore=shutil.ignore_patterns("*.golden.json"),
    )


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


def _mutate_extra_field(target_root: Path) -> None:
    """Append one derivable key to ``manifest.json`` so it has a 7th field (M3).

    The manifest is re-read, the six floor fields are re-emitted in order, then a
    single :data:`EXTRA_MANIFEST_FIELD` key is appended **after** them (so the
    reader's ``deny_unknown_fields`` parse names that key as the offending extra).
    Only ``manifest.json`` is rewritten, so the tree differs from the baseline in
    exactly this one file — and that file differs only by the one added key (M3:
    the floor is exactly the six fields, spec §0/§11; a 7th rejects).
    """
    manifest_path = target_root / "manifest.json"
    obj = json.loads(manifest_path.read_text(encoding="utf-8"))
    # Re-emit the six floor fields in order, then append the one extra key last.
    ordered = {field: obj[field] for field in MANIFEST_FIELDS}
    ordered[EXTRA_MANIFEST_FIELD] = EXTRA_MANIFEST_FIELD_VALUE
    manifest_path.write_text(json.dumps(ordered, indent=2) + "\n", encoding="utf-8")


def _mutate_empty_cadence(target_root: Path) -> None:
    """Overwrite ``manifest.json`` so ``cadence`` is the empty string (M4).

    The manifest is re-read, the single ``cadence`` value is replaced with
    :data:`EMPTY_CADENCE` (the other five floor fields byte-identical), and the
    file is rewritten with the baseline serialization (2-space indent, trailing
    newline, key order preserved). Only ``manifest.json`` is rewritten, so the
    tree differs from the baseline in exactly this one file — and that file
    differs only in the ``cadence`` value (M4: ``crs``/``cadence`` non-empty,
    spec §6.4 / §11). The M4 entry gate rejects this BEFORE ``check_m6`` rule (a)
    could fire (the M6-unreachable note in the module docstring).
    """
    manifest_path = target_root / "manifest.json"
    obj = json.loads(manifest_path.read_text(encoding="utf-8"))
    obj["cadence"] = EMPTY_CADENCE
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


# --- Bucket-B parquet/layout mutations (MS8-S3) -------------------------------
#
# Each rewrites EXACTLY ONE artifact off the baseline. The parquet rewrites read
# the baseline file as its TRUE on-disk schema (``partitioning=None`` so the hive
# ``basin`` partition column is not inferred and appended), apply one surgical
# change, and re-write with the SAME settings the baseline writer used
# (zstd + per-row-group statistics) so only the intended difference surfaces. The
# rewrites are byte-deterministic: identical input + identical pyarrow settings.

# The parquet compression the baseline scalar writer used (spec §8). Re-used here
# so a rewritten file differs from the baseline only by the surgical change.
_PARQUET_COMPRESSION = "zstd"


def _read_true_table(path: Path) -> pa.Table:
    """Read a parquet file as its TRUE file schema (no hive partition inference).

    Mirrors :func:`hdx_fixtures.assertions._read_table`: ``partitioning=None`` so
    pyarrow does not infer a ``basin`` partition column from the enclosing
    ``basin=<id>`` folder and append it — the mutation operates on exactly the
    columns the writer emitted.
    """
    return pq.read_table(path, partitioning=None)


def _write_scalar_dynamic_table(
    path: Path, table: pa.Table, *, row_group_size: int | None = None
) -> None:
    """Re-write a ``scalar_dynamic`` table with the baseline writer settings.

    Uses zstd compression with per-row-group statistics (so the Rust reader reads
    the ``time`` sort + ``basin_id`` from metadata, matching the baseline). The
    SortingColumn metadata is intentionally NOT re-emitted: the Rust reader derives
    ``time`` sortedness from the row-group statistics alone, so an honest mutation
    must not plant a stale "sorted ascending" claim. ``row_group_size`` lets a
    caller split the rows across multiple row groups (the T1 non-monotonic form).
    """
    pq.write_table(
        table,
        path,
        compression=_PARQUET_COMPRESSION,
        write_statistics=True,
        row_group_size=row_group_size,
    )


def _mutate_missing_basin_id_column(target_root: Path) -> None:
    """Drop the ``basin_id`` column from ONE basin's ``scalar_dynamic`` (I1).

    Reads :data:`_MUTATED_BASIN`'s ``scalar_dynamic.parquet``, removes the
    ``basin_id`` column, and re-writes it. ``read_scalar_dynamic`` records
    ``has_basin_id=false`` (it does NOT error), discovery sets
    ``basin_id_in_file=None`` for this basin, and ``check_i1`` ⇒ ``ran:fail``. The
    one-mutation cleanliness (steps.md §0 Bucket-B I1):

    * I3 is unaffected — ``in_file_basin_ids`` filter-maps away the ``None`` basin,
      so the surviving ids stay distinct;
    * I2 is unaffected — ``check_i2`` skips a basin with no in-file id;
    * H1 is unaffected — ``basin_id`` is never catalogued as a field.

    REJECTED ALTERNATIVE (noted, not committed): dropping ``basin_id`` from the
    root ``scalar_static`` rollup also pins I1, but the per-basin
    ``scalar_dynamic`` drop is the cleaner one-mutation (it touches a single basin
    file, not the dataset-wide rollup), confirmed I1-only before commit.
    """
    path = basin_dir(target_root, _MUTATED_BASIN) / "scalar_dynamic.parquet"
    table = _read_true_table(path)
    table = table.drop_columns(["basin_id"])
    _write_scalar_dynamic_table(path, table)


def _mutate_basin_id_folder_mismatch(target_root: Path) -> None:
    """Rewrite ONE basin's in-file ``basin_id`` to a unique foreign value (I2).

    Reads :data:`_MUTATED_BASIN`'s ``scalar_dynamic.parquet`` and replaces every
    ``basin_id`` value with :data:`I2_FOREIGN_BASIN_ID` (``9999``) — a value that
    (a) differs from the ``basin=<id>`` folder so ``check_i2`` ⇒ ``ran:fail``, and
    (b) is unique across the dataset so ``check_i3`` stays pass. I1 stays pass (the
    column is present); only the folder/in-file disagreement diverges.
    """
    path = basin_dir(target_root, _MUTATED_BASIN) / "scalar_dynamic.parquet"
    table = _read_true_table(path)
    column_index = table.schema.get_field_index("basin_id")
    foreign = pa.array([I2_FOREIGN_BASIN_ID] * table.num_rows, type=pa.string())
    table = table.set_column(
        column_index, table.schema.field("basin_id"), foreign
    )
    _write_scalar_dynamic_table(path, table)


def _mutate_ragged_field_schema(target_root: Path) -> None:
    """Rename ONE basin's ``scalar_dynamic`` data field (H1).

    Reads :data:`_MUTATED_BASIN`'s ``scalar_dynamic.parquet`` and renames the
    dynamic data field :data:`~hdx_fixtures.scalar.DYNAMIC_FIELD` (``streamflow``)
    to :data:`H1_DIVERGENT_FIELD` (``flow``), leaving dtype, nullability, quadrant,
    and every other column untouched. Only the field NAME diverges, so the basin's
    schema key differs from the reference and ``check_h1`` ⇒ ``ran:fail``. T1/I1/
    I2/I3 stay pass (``time``/``basin_id`` are unchanged).
    """
    path = basin_dir(target_root, _MUTATED_BASIN) / "scalar_dynamic.parquet"
    table = _read_true_table(path)
    new_names = [
        H1_DIVERGENT_FIELD if name == DYNAMIC_FIELD else name
        for name in table.schema.names
    ]
    table = table.rename_columns(new_names)
    _write_scalar_dynamic_table(path, table)


def _mutate_non_monotonic_time(target_root: Path) -> None:
    """Write ONE basin's ``scalar_dynamic`` ``time`` descending (T1).

    Reads :data:`_MUTATED_BASIN`'s ``scalar_dynamic.parquet``, reverses the rows so
    ``time`` descends, and re-writes the rows split across MULTIPLE row groups
    (``row_group_size`` < row count). The Rust reader derives ``time`` sortedness
    from row-group statistics: a later row group's ``time`` min is then below an
    earlier group's max, so ``time_sorted_ascending`` is ``false`` and
    ``check_t1`` ⇒ ``ran:fail`` (a single row group's min ≤ max always holds, so
    the split across groups is what makes the descending order observable in
    metadata). The column stays named ``time``, timestamp, non-nullable, so ONLY
    the sort leg diverges.

    PINNED FORM (steps.md §0 Bucket-B T1): the descending-time mutation. The
    nullable / mistyped / misnamed T1 legs are already covered in-memory by
    ``t1_negative_per_leg``.
    """
    path = basin_dir(target_root, _MUTATED_BASIN) / "scalar_dynamic.parquet"
    table = _read_true_table(path)
    n_rows = table.num_rows
    reversed_indices = pa.array(list(range(n_rows - 1, -1, -1)), type=pa.int64())
    descending = table.take(reversed_indices)
    # Split the descending rows across >=2 row groups so a later group's `time`
    # min falls below an earlier group's max (the metadata-observable descent).
    row_group_size = max(1, n_rows // 2)
    _write_scalar_dynamic_table(path, descending, row_group_size=row_group_size)


def _mutate_missing_gridded_dynamic_subtree(target_root: Path) -> None:
    """Delete ONE basin's ``gridded_dynamic/`` subtree (L2).

    Removes :data:`_MUTATED_BASIN`'s entire ``gridded_dynamic/`` directory (the
    Zarr store). ``declares_gridded_dynamic`` stays true dataset-wide (the other
    basins keep their Zarr), so ``check_l2`` requires the artifact for EVERY basin
    and this basin's empty ``dynamic_artifacts()`` ⇒ ``ran:fail``. H2 does NOT
    co-fail: ``labels_by_basin`` unions static+dynamic labels, and the surviving
    COG keeps the ``era5`` static label, so every basin's label set is ``{era5}``.

    COMMITTED FORM per the H2-collision caveat (steps.md §0 Bucket-B L2). The
    "empty ``gridded_dynamic/`` directory" alternative is rejected (an empty dir is
    not a committable git artifact and the layout walk treats it identically), so
    deleting the whole subtree is the plan of record.
    """
    subtree = gridded_dynamic_dir(basin_dir(target_root, _MUTATED_BASIN))
    shutil.rmtree(subtree)


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
    elif invalid is Invalid.EXTRA_MANIFEST_FIELD:
        _mutate_extra_field(target_root)
    elif invalid is Invalid.EMPTY_CADENCE:
        _mutate_empty_cadence(target_root)
    elif invalid is Invalid.MISSING_ROOT_ROLLUP:
        _mutate_missing_root_rollup(target_root)
    elif invalid is Invalid.MISSING_BASIN_ID_COLUMN:
        _mutate_missing_basin_id_column(target_root)
    elif invalid is Invalid.BASIN_ID_FOLDER_MISMATCH:
        _mutate_basin_id_folder_mismatch(target_root)
    elif invalid is Invalid.RAGGED_FIELD_SCHEMA:
        _mutate_ragged_field_schema(target_root)
    elif invalid is Invalid.NON_MONOTONIC_TIME:
        _mutate_non_monotonic_time(target_root)
    else:
        _mutate_missing_gridded_dynamic_subtree(target_root)

    log.info(
        "derived invalid=%s pins=%s root=%s",
        invalid.value,
        invalid.pinned_check,
        target_root,
    )
    return target_root
