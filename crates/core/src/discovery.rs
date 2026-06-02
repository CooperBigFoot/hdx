//! The **scalar half** of the shared discovery layer (architecture §3.5/§5, spec §4/§5).
//!
//! [`discover_scalar`] is the single boundary function that ties the layout walk
//! ([`walk_layout`](crate::layout::walk_layout)) and the scalar-parquet reader
//! ([`read_scalar_static`](crate::scalar_reader::read_scalar_static) /
//! [`read_scalar_dynamic`](crate::scalar_reader::read_scalar_dynamic) /
//! [`time_extent`](crate::scalar_reader::time_extent)) into one typed in-memory
//! model — [`ScalarDiscovery`] — that **both** verbs will later consume (`describe`
//! MS5 *reports* it; `validate` MS6 *checks rules over it*). It walks the basin-first
//! hive, reads every scalar artifact, and returns:
//!
//! - the **basin list** ([`ScalarDiscovery::basins`]) — the folder ids from the walk,
//!   in stable sorted order;
//! - the **scalar field catalog** ([`ScalarDiscovery::scalar_fields`]) — the
//!   homogeneous scalar schema as discovered: the [`Quadrant::ScalarStatic`] fields
//!   from `scalar_static.parquet` plus the [`Quadrant::ScalarDynamic`] fields from a
//!   representative basin (spec §5 — one-basin discovery; H1 *enforcement* across
//!   basins is MS6);
//! - the **per-basin facts** ([`ScalarDiscovery::per_basin`]) — one [`BasinScalar`]
//!   per basin: the folder-vs-in-file `basin_id` pair (recorded side by side for
//!   MS6's I2 cross-check — never compared here), the `time` descriptor, the
//!   per-basin `time` extent with its provenance, and the basin's own scalar field
//!   list;
//! - the **root-rollup presence facts** ([`ScalarDiscovery::root_rollups`]).
//!
//! ## Records facts, never a verdict (spec §14 — enforcement is MS6)
//!
//! Like the layout walk and the scalar reader it composes, this assembler **surfaces
//! gaps as facts, never a verdict**. A missing root rollup is recorded in
//! [`RootRollupPresence`] and discovery still succeeds (L1 enforcement is MS6); a
//! basin with no readable in-file `basin_id` records `basin_id_in_file == None`; the
//! three basins' extents may differ (§6.1 ragged extents) and are surfaced as facts.
//! The only failures are structural — an unreadable dataset directory, or a present
//! scalar artifact whose bytes/metadata cannot be decoded — which propagate as the
//! typed [`CoreError`] the underlying layer raised.
//!
//! ## Inert / agnostic (spec §1/§11)
//!
//! Every field of every type here is a structural fact: a basin id, an ordinary
//! [`Field`] (exactly `name`/`quadrant`/`dtype`/`units`/`grid_label`), a `time`
//! descriptor, a `[start, end]` extent with provenance, or a presence flag. There is
//! **no** transform, role, semantic type, or computation-source field, and **no**
//! manifest-floor field — the six-field [`Manifest`](crate::manifest::Manifest) is
//! untouched. Scalar fields are catalogued purely by physical schema, inheriting the
//! scalar reader's no-name-magic discipline (spec §2).
//!
//! ## The MS4 seam (the gridded / geometry half attaches here)
//!
//! This is the **scalar half** of architecture §3.5's discovery inputs. MS4 attaches
//! the gridded / geometry half **without reshaping** this model:
//!
//! - the per-basin **gridded subtree paths** are already recorded on the
//!   [`LayoutModel`](crate::layout::LayoutModel) ([`BasinDir::gridded_static`] /
//!   [`BasinDir::gridded_dynamic`]) — MS4's COG/Zarr readers consume those paths;
//! - architecture §3.5's `grids: Vec<GridInfo>` (per grid-label extent/affine/res/crs)
//!   and `delineations: Vec<DelineationLabel>` (from `outlines.geoparquet`) are the
//!   two MS4-owned additions; they sit **alongside** [`ScalarDiscovery`] in the eventual
//!   combined discovery model, not inside [`BasinScalar`]. MS3 leaves the `outlines`
//!   presence fact on [`RootRollupPresence`] as the seam MS4's geometry reader keys off.
//!
//! ## Glossary
//!
//! | Term | Meaning |
//! |---|---|
//! | scalar field catalog | the homogeneous scalar schema (static rollup + representative dynamic basin), spec §5 |
//! | folder id vs in-file `basin_id` | the locality id (`basin=<id>` dir) paired with the authoritative in-file id (spec §3); recorded for MS6 I2 |
//! | root-rollup presence | whether each dataset-level rollup (`scalar_static.parquet`, `outlines.geoparquet`) exists (spec §4/§14 L1) |

use std::path::Path;

use tracing::{debug, info, instrument};

use crate::error::CoreError;
use crate::field::Field;
use crate::layout::{BasinDir, LayoutModel, walk_layout};
use crate::newtypes::BasinId;
use crate::scalar_reader::{
    TimeColumn, TimeExtent, read_scalar_dynamic, read_scalar_static, time_extent,
};

/// The two dataset-level root-rollup **presence facts** (spec §4/§14 L1).
///
/// Carried verbatim from the layout walk: whether `scalar_static.parquet` and
/// `outlines.geoparquet` exist at the dataset root. This records *facts*, never a
/// verdict — an absent rollup is reported as `false` and discovery still succeeds
/// (L1 enforcement is MS6). The `outlines` flag is the seam MS4's geometry reader
/// keys off (the schema is parsed in MS4, not here).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RootRollupPresence {
    scalar_static: bool,
    outlines: bool,
}

impl RootRollupPresence {
    /// Returns `true` iff `scalar_static.parquet` is present at the dataset root.
    pub fn scalar_static_present(&self) -> bool {
        self.scalar_static
    }

    /// Returns `true` iff `outlines.geoparquet` is present at the dataset root
    /// (the MS4 geometry-reader seam — schema parsed in MS4, not here).
    pub fn outlines_present(&self) -> bool {
        self.outlines
    }
}

/// The discovered scalar facts of one basin (spec §3/§5/§6).
///
/// Pairs the **folder id** ([`basin_id_folder`](Self::basin_id_folder), parsed from
/// the `basin=<id>` directory — locality) with the **authoritative in-file
/// `basin_id`** ([`basin_id_in_file`](Self::basin_id_in_file), read from the column —
/// `None` if the column is absent). MS3 records the pair side by side for MS6's I2
/// cross-check; it does **not** decide agreement here.
///
/// It also carries the basin's `time` descriptor, its `time` extent (with
/// provenance), and the basin's own scalar field list. Every field is a structural
/// fact — the type is **inert/agnostic** (spec §1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BasinScalar {
    basin_id_folder: BasinId,
    basin_id_in_file: Option<BasinId>,
    time: Option<TimeColumn>,
    time_extent: Option<TimeExtent>,
    fields: Vec<Field>,
}

impl BasinScalar {
    /// Borrows the **folder id** parsed from the `basin=<id>` directory (locality).
    pub fn basin_id_folder(&self) -> &BasinId {
        &self.basin_id_folder
    }

    /// Borrows the **authoritative in-file `basin_id`** read from the column, or
    /// `None` if the column is absent / carried no value (spec §3).
    ///
    /// A conformant per-basin table holds exactly one distinct value; this records
    /// the first such value (or `None`). MS6's I2 check pairs it with
    /// [`basin_id_folder`](Self::basin_id_folder) — MS3 decides nothing.
    pub fn basin_id_in_file(&self) -> Option<&BasinId> {
        self.basin_id_in_file.as_ref()
    }

    /// Borrows the basin's `time` column descriptor, or `None` if the
    /// `scalar_dynamic.parquet` is absent for this basin (spec §6/§14 T1).
    pub fn time(&self) -> Option<&TimeColumn> {
        self.time.as_ref()
    }

    /// Returns the basin's `time` extent (with its provenance), or `None` if the
    /// `scalar_dynamic.parquet` is absent for this basin (spec §6.1).
    pub fn time_extent(&self) -> Option<TimeExtent> {
        self.time_extent
    }

    /// Borrows the basin's dynamic-scalar field catalog (ordinary [`Field`]s).
    pub fn fields(&self) -> &[Field] {
        &self.fields
    }
}

/// The **scalar half** of the shared discovery model (architecture §3.5/§5).
///
/// Produced in one call by [`discover_scalar`]; consumed by `describe` (MS5) and
/// `validate` (MS6). Holds the basin list, the homogeneous scalar field catalog, the
/// per-basin facts, and the root-rollup presence facts. It is **inert/agnostic**
/// (spec §1): every field is a structural fact, and it adds no manifest-floor or
/// derivable field. MS4 attaches the gridded / geometry half alongside it without
/// reshaping it (see the module-level **MS4 seam**).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScalarDiscovery {
    basins: Vec<BasinId>,
    scalar_fields: Vec<Field>,
    per_basin: Vec<BasinScalar>,
    root_rollups: RootRollupPresence,
    scalar_static_has_basin_id: Option<bool>,
}

impl ScalarDiscovery {
    /// Borrows the discovered basin list (folder ids), in stable sorted order.
    pub fn basins(&self) -> &[BasinId] {
        &self.basins
    }

    /// Borrows the homogeneous scalar field catalog (spec §5).
    ///
    /// The [`Quadrant::ScalarStatic`](crate::field::Quadrant::ScalarStatic) fields
    /// from `scalar_static.parquet` followed by the
    /// [`Quadrant::ScalarDynamic`](crate::field::Quadrant::ScalarDynamic) fields from
    /// a representative basin (one-basin discovery; H1 enforcement across basins is
    /// MS6). The fields are ordinary — no name-pattern special-casing (spec §2).
    pub fn scalar_fields(&self) -> &[Field] {
        &self.scalar_fields
    }

    /// Borrows the per-basin facts, one [`BasinScalar`] per basin in basin order.
    pub fn per_basin(&self) -> &[BasinScalar] {
        &self.per_basin
    }

    /// Returns the root-rollup presence facts (spec §4/§14 L1).
    pub fn root_rollups(&self) -> RootRollupPresence {
        self.root_rollups
    }

    /// Returns whether `scalar_static.parquet`'s `basin_id` column is present
    /// (spec §3/§14 I1), or `None` when the static rollup is absent.
    ///
    /// **Additive accessor (the MS6-S2 I1 static-rollup seam).** The presence fact is
    /// read inside [`discover_scalar`] via
    /// [`ScalarStaticTable::has_basin_id`](crate::scalar_reader::ScalarStaticTable::has_basin_id)
    /// but was not surfaced on this model until MS6 needed it for I1. This accessor only
    /// **exposes** the already-read fact — it is **never** a reshape of the MS3 contract
    /// (the four original accessors are untouched). `None` distinguishes "the rollup was
    /// absent, so the column-presence question does not apply here" (that absence is an
    /// L1 concern) from `Some(false)` ("the rollup is present but lacks the column").
    pub fn scalar_static_has_basin_id(&self) -> Option<bool> {
        self.scalar_static_has_basin_id
    }
}

/// Reads one basin's `scalar_dynamic.parquet` into a [`BasinScalar`] (spec §3/§5/§6).
///
/// When the artifact is **absent**, the basin is recorded as a fact with no in-file
/// id, no `time` descriptor, no extent, and an empty field list — discovery does not
/// fail (L2/T1 enforcement is MS6). When the artifact is **present**, its bytes are
/// read for the scalar field catalog, the in-file `basin_id` (the first distinct
/// value, paired with the folder id for MS6's I2), the `time` descriptor, and the
/// `time` extent (with provenance).
///
/// # Errors
///
/// | Condition | Error |
/// |---|---|
/// | the present `scalar_dynamic.parquet` cannot be read or its metadata fails to decode | [`CoreError::ParquetRead`] |
/// | the present table is missing its structurally required `time` column | [`CoreError::MissingScalarColumn`] |
/// | a data column's arrow type does not map to a supported dtype | [`CoreError::UnknownDtype`] |
fn discover_basin(basin: &BasinDir) -> Result<BasinScalar, CoreError> {
    let folder_id = basin.folder_id().clone();

    // The artifact may legitimately be absent (recorded as a fact; L2 is MS6).
    if !basin.scalar_dynamic().is_present() {
        debug!(
            basin = folder_id.as_str(),
            "scalar_dynamic.parquet absent; recorded as a fact"
        );
        return Ok(BasinScalar {
            basin_id_folder: folder_id,
            basin_id_in_file: None,
            time: None,
            time_extent: None,
            fields: Vec::new(),
        });
    }

    let path = basin.scalar_dynamic().path();
    let table = read_scalar_dynamic(path)?;
    let extent = time_extent(path)?;

    // The authoritative in-file id: the first distinct value, or `None`. MS6's I2
    // cross-check pairs this with the folder id — MS3 records, never compares.
    let basin_id_in_file = table.basin_id_values().first().cloned();

    debug!(
        basin = folder_id.as_str(),
        in_file = basin_id_in_file.as_ref().map(BasinId::as_str),
        fields = table.fields().len(),
        "discovered basin scalar facts"
    );

    Ok(BasinScalar {
        basin_id_folder: folder_id,
        basin_id_in_file,
        time: Some(table.time().clone()),
        time_extent: Some(extent),
        fields: table.fields().to_vec(),
    })
}

/// Assembles the homogeneous scalar field catalog from the static + dynamic facts.
///
/// The [`Quadrant::ScalarStatic`](crate::field::Quadrant::ScalarStatic) fields read
/// from `scalar_static.parquet` (empty if the rollup is absent), followed by the
/// [`Quadrant::ScalarDynamic`](crate::field::Quadrant::ScalarDynamic) fields of the
/// first basin that exposed any (spec §5 — a representative one-basin read). H1
/// enforcement across basins is MS6; here we surface the discovered schema as a fact.
fn assemble_field_catalog(static_fields: &[Field], per_basin: &[BasinScalar]) -> Vec<Field> {
    let mut catalog: Vec<Field> = static_fields.to_vec();
    if let Some(first_dynamic) = per_basin.iter().find(|b| !b.fields().is_empty()) {
        catalog.extend(first_dynamic.fields().iter().cloned());
    }
    catalog
}

/// Discovers the **scalar half** of the shared discovery model in one call
/// (architecture §3.5/§5, spec §4/§5/§6).
///
/// Walks the basin-first hive ([`walk_layout`]), reads `scalar_static.parquet` (if
/// present) and each basin's `scalar_dynamic.parquet`, and assembles a
/// [`ScalarDiscovery`]: the basin list, the homogeneous scalar field catalog, the
/// per-basin facts (folder-vs-in-file `basin_id` pair, `time` descriptor + extent
/// with provenance, basin field list), and the root-rollup presence facts.
///
/// **Surfaces gaps as facts, never a verdict.** An absent root rollup is recorded
/// (L1 is MS6); an absent per-basin `scalar_dynamic.parquet` yields a `BasinScalar`
/// with no in-file id / descriptor / extent; ragged extents across basins (§6.1) are
/// surfaced. Only structural failures (unreadable directory, undecodable present
/// artifact) propagate as errors.
///
/// # Errors
///
/// | Condition | Error |
/// |---|---|
/// | the dataset `path` is not a readable directory | [`CoreError::LayoutWalk`] |
/// | a present scalar artifact cannot be read or its metadata fails to decode | [`CoreError::ParquetRead`] |
/// | a present `scalar_dynamic.parquet` is missing its required `time` column | [`CoreError::MissingScalarColumn`] |
/// | a present scalar artifact carries a column whose arrow type is unsupported | [`CoreError::UnknownDtype`] |
#[instrument(fields(path = %path.as_ref().display()))]
pub fn discover_scalar(path: impl AsRef<Path>) -> Result<ScalarDiscovery, CoreError> {
    let layout: LayoutModel = walk_layout(path)?;

    let root_rollups = RootRollupPresence {
        scalar_static: layout.scalar_static().is_present(),
        outlines: layout.outlines().is_present(),
    };

    // The dataset-level static rollup: read its fields if present, else an empty
    // catalog (its absence is a fact in `root_rollups`; L1 is MS6). The `basin_id`
    // column-presence fact (the MS6 I1 static-rollup leg) is captured here when the
    // rollup is present, and surfaced additively on the model — `None` when absent.
    let (static_fields, scalar_static_has_basin_id): (Vec<Field>, Option<bool>) =
        if layout.scalar_static().is_present() {
            let table = read_scalar_static(layout.scalar_static().path())?;
            (table.fields().to_vec(), Some(table.has_basin_id()))
        } else {
            (Vec::new(), None)
        };

    let basins: Vec<BasinId> = layout
        .basins()
        .iter()
        .map(|b| b.folder_id().clone())
        .collect();

    let mut per_basin: Vec<BasinScalar> = Vec::with_capacity(layout.basins().len());
    for basin in layout.basins() {
        per_basin.push(discover_basin(basin)?);
    }

    let scalar_fields = assemble_field_catalog(&static_fields, &per_basin);

    info!(
        basins = basins.len(),
        scalar_fields = scalar_fields.len(),
        scalar_static = root_rollups.scalar_static_present(),
        outlines = root_rollups.outlines_present(),
        "assembled the scalar half of the discovery model"
    );

    Ok(ScalarDiscovery {
        basins,
        scalar_fields,
        per_basin,
        root_rollups,
        scalar_static_has_basin_id,
    })
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use time::OffsetDateTime;
    use time::format_description::well_known::Rfc3339;

    use crate::discovery::{ScalarDiscovery, discover_scalar};
    use crate::field::{Dtype, Quadrant};
    use crate::newtypes::BasinId;
    use crate::scalar_reader::TimeExtentSource;

    /// Resolves a path under the committed `conformance/` fixture tree.
    ///
    /// `CARGO_MANIFEST_DIR` is `crates/core`; the fixtures live two levels up at the
    /// workspace root, so discovery runs against the real MS2 trees.
    fn conformance(rel: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../conformance")
            .join(rel)
    }

    /// Parses an RFC 3339 datetime for asserting exact time-extent boundaries.
    fn rfc3339(s: &str) -> OffsetDateTime {
        OffsetDateTime::parse(s, &Rfc3339).expect("test datetime must parse")
    }

    #[test]
    fn discover_scalar_populates_the_whole_scalar_half_from_the_valid_fixture() {
        let discovery: ScalarDiscovery = discover_scalar(conformance("valid/minimal"))
            .expect("the valid fixture must discover without error");

        // Basin list: exactly the three basins, in stable sorted order.
        let ids: Vec<&str> = discovery.basins().iter().map(BasinId::as_str).collect();
        assert_eq!(ids, vec!["0001", "0002", "0003"]);

        // Both root rollups present (recorded facts).
        assert!(discovery.root_rollups().scalar_static_present());
        assert!(discovery.root_rollups().outlines_present());

        // Scalar field catalog: {drainage_area: ScalarStatic/f64, streamflow:
        // ScalarDynamic/f64} — static rollup field then the representative dynamic.
        assert_eq!(discovery.scalar_fields().len(), 2);
        let drainage = &discovery.scalar_fields()[0];
        assert_eq!(drainage.name().as_str(), "drainage_area");
        assert_eq!(drainage.quadrant(), Quadrant::ScalarStatic);
        assert_eq!(drainage.dtype(), Dtype::F64);
        let streamflow = &discovery.scalar_fields()[1];
        assert_eq!(streamflow.name().as_str(), "streamflow");
        assert_eq!(streamflow.quadrant(), Quadrant::ScalarDynamic);
        assert_eq!(streamflow.dtype(), Dtype::F64);

        // One BasinScalar per basin, in basin order.
        assert_eq!(discovery.per_basin().len(), 3);
        let folder_ids: Vec<&str> = discovery
            .per_basin()
            .iter()
            .map(|b| b.basin_id_folder().as_str())
            .collect();
        assert_eq!(folder_ids, vec!["0001", "0002", "0003"]);

        for basin in discovery.per_basin() {
            // I2 SEAM (documented, NOT enforced): the folder id and the in-file id
            // are recorded as a pair. The test asserts them equal to document the
            // seam — MS3 does NOT enforce agreement (that is MS6).
            assert_eq!(
                basin.basin_id_in_file(),
                Some(basin.basin_id_folder()),
                "folder id and in-file id coincide on the conformant fixture (I2 seam)"
            );

            // T1 inputs: a non-nullable, sorted `time` descriptor (recorded, not enforced).
            let time = basin
                .time()
                .expect("each basin has a time descriptor on the conformant fixture");
            assert_eq!(time.name(), "time");
            assert_eq!(time.dtype(), Dtype::Timestamp);
            assert!(!time.is_nullable(), "fixture time is non-nullable");
            assert!(time.is_sorted_ascending(), "fixture time is sorted ascending");

            // The extent comes from row-group statistics (not the bounded fallback).
            let extent = basin
                .time_extent()
                .expect("each basin has a time extent on the conformant fixture");
            assert_eq!(extent.source(), TimeExtentSource::Statistics);

            // Each basin carries its one ordinary ScalarDynamic field.
            assert_eq!(basin.fields().len(), 1);
            assert_eq!(basin.fields()[0].name().as_str(), "streamflow");
            assert_eq!(basin.fields()[0].quadrant(), Quadrant::ScalarDynamic);
        }
    }

    #[test]
    fn ragged_extents_across_basins_are_surfaced_as_facts() {
        let discovery = discover_scalar(conformance("valid/minimal"))
            .expect("the valid fixture must discover");

        let extents: Vec<_> = discovery
            .per_basin()
            .iter()
            .map(|b| {
                b.time_extent()
                    .expect("each basin has an extent on the conformant fixture")
            })
            .collect();

        // §6.1: the three basins span entirely different periods of record.
        assert_eq!(
            extents[0].start().as_offset_date_time(),
            rfc3339("2000-01-01T00:00:00Z")
        );
        assert_eq!(
            extents[0].end().as_offset_date_time(),
            rfc3339("2000-01-05T00:00:00Z")
        );
        assert_eq!(
            extents[1].start().as_offset_date_time(),
            rfc3339("2010-06-15T00:00:00Z")
        );
        assert_eq!(
            extents[2].start().as_offset_date_time(),
            rfc3339("2005-03-01T00:00:00Z")
        );

        // Surfaced as facts: the three extents genuinely differ (not enforced).
        assert_ne!(extents[0].end(), extents[1].end());
        assert_ne!(extents[1].end(), extents[2].end());
        assert_ne!(extents[0].end(), extents[2].end());
    }

    #[test]
    fn missing_root_rollup_discovers_successfully_with_the_gap_recorded() {
        // Gaps-as-facts: discovery SUCCEEDS (no verdict) on the missing-root-rollup
        // tree and reports the absent rollup + the present basins. L1 is MS6.
        let discovery = discover_scalar(conformance("invalid/missing-root-rollup"))
            .expect("discovery must SUCCEED and record the gap (L1 enforcement is MS6)");

        // `outlines.geoparquet` is the absent rollup in this fixture.
        assert!(discovery.root_rollups().scalar_static_present());
        assert!(
            !discovery.root_rollups().outlines_present(),
            "the absent rollup is recorded as a fact, not raised"
        );

        // The present basins still enumerate and read.
        let ids: Vec<&str> = discovery.basins().iter().map(BasinId::as_str).collect();
        assert_eq!(ids, vec!["0001", "0002", "0003"]);
        assert_eq!(discovery.per_basin().len(), 3);
    }

    #[test]
    fn assembled_scalar_fields_are_ordinary_no_name_magic_no_provenance() {
        // The assembled catalog carries only inert/agnostic facts: name, quadrant,
        // dtype, units, grid_label. There is no role/transform/semantic/provenance
        // surface to read, and no name-pattern special-casing (spec §1/§2). The
        // Field type structurally cannot carry such a field — this asserts the
        // catalog is exactly the two ordinary fields with their verbatim names.
        let discovery = discover_scalar(conformance("valid/minimal"))
            .expect("the valid fixture must discover");

        for field in discovery.scalar_fields() {
            // Scalar fields carry no grid label (the only conditional Field datum).
            assert_eq!(
                field.grid_label(),
                None,
                "a scalar field carries no grid label and no other derived datum"
            );
            // Units are recorded as absent (parquet column metadata carries none in
            // the fixture) — never invented (spec §1).
            assert_eq!(field.units().as_deref(), None);
        }

        let names: Vec<&str> = discovery
            .scalar_fields()
            .iter()
            .map(|f| f.name().as_str())
            .collect();
        assert_eq!(
            names,
            vec!["drainage_area", "streamflow"],
            "names are verbatim producer strings — no suffix/prefix magic"
        );
    }

    /// Compile-level seam check: [`ScalarDiscovery`] exposes exactly the scalar-half
    /// data MS4 extends. The gridded `GridInfo` + `delineations` (architecture §3.5)
    /// attach **alongside** this model — the per-basin gridded subtree paths already
    /// live on the `LayoutModel`, and the `outlines` presence fact is the geometry
    /// reader's seam. This test pins the four scalar-half accessors so MS4 cannot
    /// silently reshape them.
    #[test]
    fn scalar_discovery_exposes_the_ms4_seam_accessors() {
        let discovery = discover_scalar(conformance("valid/minimal"))
            .expect("the valid fixture must discover");

        // The four scalar-half accessors MS4 builds on (compile + shape check).
        let _basins: &[BasinId] = discovery.basins();
        let _fields: &[crate::field::Field] = discovery.scalar_fields();
        let _per_basin: &[crate::discovery::BasinScalar] = discovery.per_basin();
        let rollups = discovery.root_rollups();
        // The `outlines` presence is the MS4 geometry-reader seam.
        assert!(rollups.outlines_present());
    }
}
