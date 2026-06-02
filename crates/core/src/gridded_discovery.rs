//! The **gridded / geometry half** of the shared discovery layer, and the combined
//! [`Discovery`] model both verbs consume (architecture §3.5/§5, spec §4/§7/§8/§9).
//!
//! [`discover_gridded`] is the single boundary function that ties the layout walk
//! ([`walk_layout`](crate::layout::walk_layout)) and the three MS4 readers — the COG
//! reader ([`read_cog_grid`](crate::cog_reader::read_cog_grid)), the Zarr reader
//! ([`read_zarr_grid`](crate::zarr_reader::read_zarr_grid)), and the geoparquet reader
//! ([`read_outlines`](crate::geoparquet_reader::read_outlines)) — into one typed
//! in-memory model ([`GriddedDiscovery`]) that sits **alongside** MS3's
//! [`ScalarDiscovery`](crate::discovery::ScalarDiscovery). The [`Discovery`] struct
//! **pairs** the two halves without reshaping either, so MS5 (`describe`) and MS6
//! (`validate`) consume **one** model.
//!
//! It walks the basin-first hive and, for each basin with a present `gridded_static`
//! / `gridded_dynamic` subtree, enumerates the `<label>.tif` / `<label>.zarr`
//! artifacts (the grid label is the artifact file stem — HDX names nothing from the
//! file *contents*), calls the COG / Zarr reader per artifact, and reads
//! `outlines.geoparquet` once (when present). It returns:
//!
//! - the per-grid-label representative geometries ([`GriddedDiscovery::grids`]) — the
//!   COG [`GridInfo`](crate::grid::GridInfo) and the Zarr [`GridInfo`] for the shared
//!   `era5` label, recorded side by side;
//! - the homogeneous gridded field catalog ([`GriddedDiscovery::gridded_fields`]) —
//!   the `GriddedStatic` band fields plus the `GriddedDynamic` data-variable fields
//!   from a representative basin (spec §5 — one-basin discovery; cross-basin
//!   enforcement is MS6);
//! - the distinct delineation labels ([`GriddedDiscovery::delineations`]) from
//!   `outlines.geoparquet` (spec §9);
//! - the per-basin observed facts ([`GriddedDiscovery::per_basin`]) — one
//!   [`BasinGridded`] per basin: the grid labels observed in *each* subtree (the
//!   **G2 precondition fact**), the static/dynamic [`GridInfo`]s, and the Zarr
//!   consolidated-metadata path taken (MED-5).
//!
//! ## The G2 alignment precondition is *observed*, never enforced (spec §8/§14 G2)
//!
//! When the *same* grid label appears across the `gridded_static` (COG) and
//! `gridded_dynamic` (Zarr) subtrees, spec §8 says the two artifacts are
//! cell-for-cell aligned. MS4 **records that fact** — the shared label and the two
//! [`GridInfo`] extents — so MS6 can *enforce* alignment. Because both readers build
//! their [`GridExtent`](crate::grid::GridExtent) in the single cell-edge convention
//! ([`grid`](crate::grid)), two genuinely-aligned artifacts now yield **identical**
//! extents (`10.0`/`50.0` on the fixture) — the prior structural-misread defect's
//! end-to-end fix. MS4 observes this; it renders **no verdict** (G2 enforcement is
//! MS6).
//!
//! ## Records facts, never a verdict (spec §14 — enforcement is MS6)
//!
//! Like the scalar assembler it parallels, this surfaces **gaps as facts**. A basin
//! with no `gridded_static` / `gridded_dynamic` subtree records an *empty*
//! [`BasinGridded`] and discovery still succeeds; an absent `outlines.geoparquet`
//! yields an empty delineation list. Only structural failures — an unreadable dataset
//! directory, or a *present* gridded artifact whose metadata cannot be decoded —
//! propagate as the typed [`CoreError`] the underlying reader raised.
//!
//! ## Inert / agnostic (spec §1/§11)
//!
//! Every field here is a structural fact: a [`GridInfo`], an ordinary
//! [`Field`](crate::field::Field), a [`GridLabel`], a [`DelineationLabel`], or the
//! recorded Zarr path ([`ConsolidatedMetadataSource`]). There is **no** transform,
//! role, semantic type, or provenance, and **no** manifest-floor field — the
//! six-field [`Manifest`](crate::manifest::Manifest) is untouched.
//!
//! ## Glossary
//!
//! | Term | Meaning |
//! |---|---|
//! | gridded field catalog | the homogeneous gridded schema (`GriddedStatic` bands + a representative basin's `GriddedDynamic` vars), spec §5 |
//! | shared grid label ⇒ alignment | a label seen in *both* gridded subtrees signals cell-for-cell alignment (spec §8); the G2 precondition |
//! | G2 precondition fact | the per-basin observed grid labels + coinciding extents MS6 enforces G2 over |
//! | MED-5 Zarr path | which path the Zarr reader took (consolidated/live vs an R3 skip), recorded for honest downstream reporting |

use std::ffi::OsStr;
use std::path::Path;

use tracing::{debug, info, instrument};

use crate::cog_reader::{CogGrid, read_cog_grid};
use crate::discovery::{ScalarDiscovery, discover_scalar};
use crate::error::CoreError;
use crate::field::Field;
use crate::geoparquet_reader::{OutlinesInfo, read_outlines};
use crate::grid::GridInfo;
use crate::layout::{BasinDir, LayoutModel, walk_layout};
use crate::newtypes::{BasinId, DelineationLabel, GridLabel};
use crate::zarr_reader::{ConsolidatedMetadataSource, ZarrGrid, read_zarr_grid};

/// The file extension of a `gridded_static` COG artifact (`<label>.tif`).
const COG_EXTENSION: &str = "tif";
/// The file extension of a `gridded_dynamic` Zarr store (`<label>.zarr`).
const ZARR_EXTENSION: &str = "zarr";

/// The discovered gridded·static facts of one COG artifact (spec §7/§8).
///
/// Pairs the artifact's grid label (its file stem — HDX names nothing from the file
/// contents) with the per-artifact [`GridInfo`]. Inert/agnostic: geometry + a label.
#[derive(Debug, Clone, PartialEq)]
pub struct StaticArtifact {
    grid_label: GridLabel,
    grid_info: GridInfo,
}

impl StaticArtifact {
    /// Borrows the grid label (the COG file stem).
    pub fn grid_label(&self) -> &GridLabel {
        &self.grid_label
    }

    /// Borrows the per-artifact grid geometry.
    pub fn grid_info(&self) -> &GridInfo {
        &self.grid_info
    }
}

/// The discovered gridded·dynamic facts of one Zarr store (spec §7/§8, MED-5).
///
/// Pairs the store's grid label (its file stem) with the per-store [`GridInfo`] and
/// the [`ConsolidatedMetadataSource`] path the Zarr reader took (recorded for honest
/// downstream reporting). Inert/agnostic: geometry + a label + the path taken.
#[derive(Debug, Clone, PartialEq)]
pub struct DynamicArtifact {
    grid_label: GridLabel,
    grid_info: GridInfo,
    consolidated_source: ConsolidatedMetadataSource,
}

impl DynamicArtifact {
    /// Borrows the grid label (the Zarr store file stem).
    pub fn grid_label(&self) -> &GridLabel {
        &self.grid_label
    }

    /// Borrows the per-store grid geometry.
    pub fn grid_info(&self) -> &GridInfo {
        &self.grid_info
    }

    /// Borrows the consolidated-metadata path the Zarr reader took (MED-5).
    pub fn consolidated_source(&self) -> &ConsolidatedMetadataSource {
        &self.consolidated_source
    }
}

/// The discovered gridded facts of one basin (spec §4/§7/§8, feeds §14 G2).
///
/// Records, per basin, the grid labels observed in **each** gridded subtree — the
/// **G2 precondition fact** (a label seen in *both* subtrees signals cell-for-cell
/// alignment, spec §8). Holds the static / dynamic artifacts (each with its
/// [`GridInfo`]) so MS6 can compare the two subtrees' extents for the shared label.
///
/// A basin with no gridded subtree records empty artifact lists — a fact, not a
/// verdict (the gaps-as-facts discipline; L2 enforcement is MS6). Inert/agnostic.
#[derive(Debug, Clone, PartialEq)]
pub struct BasinGridded {
    basin_id_folder: BasinId,
    static_artifacts: Vec<StaticArtifact>,
    dynamic_artifacts: Vec<DynamicArtifact>,
}

impl BasinGridded {
    /// Borrows the folder id of the basin these gridded facts belong to.
    pub fn basin_id_folder(&self) -> &BasinId {
        &self.basin_id_folder
    }

    /// Borrows the COG (`gridded_static`) artifacts observed in this basin.
    pub fn static_artifacts(&self) -> &[StaticArtifact] {
        &self.static_artifacts
    }

    /// Borrows the Zarr (`gridded_dynamic`) artifacts observed in this basin.
    pub fn dynamic_artifacts(&self) -> &[DynamicArtifact] {
        &self.dynamic_artifacts
    }

    /// Returns the distinct grid labels observed in the `gridded_static` subtree.
    ///
    /// First-seen order; one of the two halves of the **G2 precondition fact** (the
    /// other being [`dynamic_grid_labels`](Self::dynamic_grid_labels)). MS6 enforces
    /// that a label seen in *both* implies coinciding extents.
    pub fn static_grid_labels(&self) -> Vec<&GridLabel> {
        self.static_artifacts
            .iter()
            .map(StaticArtifact::grid_label)
            .collect()
    }

    /// Returns the distinct grid labels observed in the `gridded_dynamic` subtree.
    ///
    /// First-seen order; the second half of the **G2 precondition fact** (see
    /// [`static_grid_labels`](Self::static_grid_labels)).
    pub fn dynamic_grid_labels(&self) -> Vec<&GridLabel> {
        self.dynamic_artifacts
            .iter()
            .map(DynamicArtifact::grid_label)
            .collect()
    }
}

/// The **gridded / geometry half** of the shared discovery model (architecture §3.5).
///
/// Produced in one call by [`discover_gridded`]; paired with
/// [`ScalarDiscovery`](crate::discovery::ScalarDiscovery) in the combined
/// [`Discovery`]. Holds the per-grid-label representative geometries, the homogeneous
/// gridded field catalog, the distinct delineation labels, and the per-basin observed
/// facts (the G2 precondition + the MED-5 Zarr path). It is **inert/agnostic** (spec
/// §1): every field is a structural fact, and it adds no manifest-floor or derivable
/// field. It records facts; it enforces nothing.
#[derive(Debug, Clone, PartialEq)]
pub struct GriddedDiscovery {
    grids: Vec<GridInfo>,
    gridded_fields: Vec<Field>,
    delineations: Vec<DelineationLabel>,
    per_basin: Vec<BasinGridded>,
    outlines: Option<OutlinesInfo>,
}

impl GriddedDiscovery {
    /// Borrows the per-grid representative geometries (one [`GridInfo`] per artifact
    /// for the shared label, COG then Zarr), in basin → static → dynamic order.
    pub fn grids(&self) -> &[GridInfo] {
        &self.grids
    }

    /// Borrows the homogeneous gridded field catalog (spec §5).
    ///
    /// The [`Quadrant::GriddedStatic`](crate::field::Quadrant::GriddedStatic) band
    /// fields followed by the
    /// [`Quadrant::GriddedDynamic`](crate::field::Quadrant::GriddedDynamic)
    /// data-variable fields from a representative basin (one-basin discovery;
    /// cross-basin enforcement is MS6). The fields are ordinary — no name-pattern
    /// special-casing (spec §2).
    pub fn gridded_fields(&self) -> &[Field] {
        &self.gridded_fields
    }

    /// Borrows the distinct delineation labels from `outlines.geoparquet` (spec §9),
    /// or an empty slice when the outlines rollup is absent (a recorded gap).
    pub fn delineations(&self) -> &[DelineationLabel] {
        &self.delineations
    }

    /// Borrows the per-basin observed gridded facts, one [`BasinGridded`] per basin
    /// in basin order (the G2 precondition + the MED-5 Zarr path).
    pub fn per_basin(&self) -> &[BasinGridded] {
        &self.per_basin
    }

    /// Borrows the discovered `outlines.geoparquet` facts, or `None` when the outlines
    /// rollup is absent (a recorded gap; L1 is the absence's concern, not Geo1).
    ///
    /// **Additive accessor (the MS6-S2 Geo1 / I1-outlines / M5-outlines seam).** The
    /// full [`OutlinesInfo`] (column presence, the not-partitioned fact, the recorded
    /// [`Crs`](crate::newtypes::Crs)) is read inside [`discover_gridded`] but only its
    /// `delineations()` were surfaced until MS6 needed the rest. This accessor only
    /// **exposes** the already-read facts — it is **never** a reshape of the MS4
    /// contract (the four original accessors, including [`delineations`](Self::delineations),
    /// are untouched and still populated identically).
    pub fn outlines(&self) -> Option<&OutlinesInfo> {
        self.outlines.as_ref()
    }
}

/// Returns the file stem of `path` as a [`GridLabel`] when it has the expected
/// `extension`, else `None` (so non-artifact entries are skipped, not failed).
///
/// The grid label is the artifact's file stem (`era5.tif` → `era5`,
/// `era5.zarr` → `era5`) — HDX names the *label* from the artifact name, never from
/// the file contents (spec §8).
fn artifact_label(path: &Path, extension: &str) -> Option<GridLabel> {
    let ext_matches = path
        .extension()
        .and_then(OsStr::to_str)
        .is_some_and(|e| e == extension);
    if !ext_matches {
        return None;
    }
    path.file_stem().and_then(OsStr::to_str).map(GridLabel::new)
}

/// Enumerates the immediate children of a present gridded subtree, sorted by name.
///
/// Returns an empty vec when the subtree is absent (gaps-as-facts) — the caller never
/// needs to special-case presence. Skips hidden / OS-cruft entries (a leading `.`).
///
/// # Errors
///
/// | Condition | Error |
/// |---|---|
/// | the subtree directory is present but cannot be enumerated | [`CoreError::LayoutWalk`] |
fn list_subtree(subtree: &Path) -> Result<Vec<std::path::PathBuf>, CoreError> {
    if !subtree.is_dir() {
        return Ok(Vec::new());
    }
    let entries = std::fs::read_dir(subtree).map_err(|e| CoreError::LayoutWalk {
        path: subtree.display().to_string(),
        detail: e.to_string(),
    })?;
    let mut paths: Vec<std::path::PathBuf> = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| CoreError::LayoutWalk {
            path: subtree.display().to_string(),
            detail: e.to_string(),
        })?;
        let path = entry.path();
        // Skip hidden / OS-cruft entries (mirrors the layout walk's cruft filter).
        let is_cruft = path
            .file_name()
            .and_then(OsStr::to_str)
            .is_some_and(|n| n.starts_with('.'));
        if is_cruft {
            continue;
        }
        paths.push(path);
    }
    paths.sort();
    Ok(paths)
}

/// Discovers one basin's gridded facts into a [`BasinGridded`] (spec §4/§7/§8).
///
/// Enumerates the present `gridded_static` subtree for `<label>.tif` COGs (each read
/// via [`read_cog_grid`]) and the present `gridded_dynamic` subtree for
/// `<label>.zarr` stores (each read via [`read_zarr_grid`]). An absent subtree yields
/// an empty artifact list — a recorded fact, not a failure (gaps-as-facts).
///
/// # Errors
///
/// | Condition | Error |
/// |---|---|
/// | a present gridded subtree cannot be enumerated | [`CoreError::LayoutWalk`] |
/// | a present `<label>.tif` COG's metadata cannot be decoded | [`CoreError::CogRead`] / [`CoreError::MissingGridGeoref`] / [`CoreError::UnknownDtype`] |
/// | a present `<label>.zarr` store's metadata cannot be decoded | [`CoreError::ZarrRead`] / [`CoreError::MissingGridGeoref`] / [`CoreError::MissingGriddedCoordinate`] / [`CoreError::UnknownDtype`] |
fn discover_basin_gridded(basin: &BasinDir) -> Result<BasinGridded, CoreError> {
    let folder_id = basin.folder_id().clone();

    // gridded_static: each `<label>.tif` → one COG read (edge-based GridInfo + band).
    let mut static_artifacts: Vec<StaticArtifact> = Vec::new();
    if basin.gridded_static().is_present() {
        for path in list_subtree(basin.gridded_static().path())? {
            let Some(grid_label) = artifact_label(&path, COG_EXTENSION) else {
                continue;
            };
            let cog: CogGrid = read_cog_grid(&path, grid_label.clone())?;
            static_artifacts.push(StaticArtifact {
                grid_label,
                grid_info: cog.grid_info().clone(),
            });
        }
    }

    // gridded_dynamic: each `<label>.zarr` → one Zarr read (center→edge GridInfo +
    // data-var fields + the MED-5 path taken).
    let mut dynamic_artifacts: Vec<DynamicArtifact> = Vec::new();
    if basin.gridded_dynamic().is_present() {
        for path in list_subtree(basin.gridded_dynamic().path())? {
            let Some(grid_label) = artifact_label(&path, ZARR_EXTENSION) else {
                continue;
            };
            let zarr: ZarrGrid = read_zarr_grid(&path, grid_label.clone())?;
            dynamic_artifacts.push(DynamicArtifact {
                grid_label,
                grid_info: zarr.grid_info().clone(),
                consolidated_source: zarr.consolidated_source().clone(),
            });
        }
    }

    debug!(
        basin = folder_id.as_str(),
        static_artifacts = static_artifacts.len(),
        dynamic_artifacts = dynamic_artifacts.len(),
        "discovered basin gridded facts"
    );

    Ok(BasinGridded {
        basin_id_folder: folder_id,
        static_artifacts,
        dynamic_artifacts,
    })
}

/// Assembles the homogeneous gridded field catalog from the first basin to expose
/// each gridded·static / gridded·dynamic artifact (spec §5 — one-basin discovery).
///
/// The first static artifact's band field(s) followed by the first dynamic artifact's
/// data-variable field(s). Because the COG reader exposes the band field on the
/// [`CogGrid`] and the Zarr reader exposes the data-var fields on the [`ZarrGrid`],
/// this re-reads the representative artifacts once to recover their fields (the
/// per-basin model carries only the geometry). Cross-basin H1 enforcement is MS6.
///
/// # Errors
///
/// Propagates any reader error from re-reading the representative artifacts.
fn assemble_gridded_field_catalog(
    layout: &LayoutModel,
    per_basin: &[BasinGridded],
) -> Result<Vec<Field>, CoreError> {
    let mut catalog: Vec<Field> = Vec::new();

    // The first basin exposing a static artifact contributes the band field(s).
    if let Some((basin, artifact)) = per_basin
        .iter()
        .find_map(|b| b.static_artifacts().first().map(|a| (b, a)))
    {
        let path = static_artifact_path(layout, basin.basin_id_folder(), artifact.grid_label());
        let cog = read_cog_grid(&path, artifact.grid_label().clone())?;
        catalog.push(cog.field().clone());
    }

    // The first basin exposing a dynamic artifact contributes the data-var field(s).
    if let Some((basin, artifact)) = per_basin
        .iter()
        .find_map(|b| b.dynamic_artifacts().first().map(|a| (b, a)))
    {
        let path = dynamic_artifact_path(layout, basin.basin_id_folder(), artifact.grid_label());
        let zarr = read_zarr_grid(&path, artifact.grid_label().clone())?;
        catalog.extend(zarr.fields().iter().cloned());
    }

    Ok(catalog)
}

/// Resolves the on-disk path of a basin's `gridded_static/<label>.tif` COG.
fn static_artifact_path(
    layout: &LayoutModel,
    basin: &BasinId,
    label: &GridLabel,
) -> std::path::PathBuf {
    layout
        .root()
        .join(format!("basin={}", basin.as_str()))
        .join("gridded_static")
        .join(format!("{}.{}", label.as_str(), COG_EXTENSION))
}

/// Resolves the on-disk path of a basin's `gridded_dynamic/<label>.zarr` store.
fn dynamic_artifact_path(
    layout: &LayoutModel,
    basin: &BasinId,
    label: &GridLabel,
) -> std::path::PathBuf {
    layout
        .root()
        .join(format!("basin={}", basin.as_str()))
        .join("gridded_dynamic")
        .join(format!("{}.{}", label.as_str(), ZARR_EXTENSION))
}

/// Discovers the **gridded / geometry half** of the shared discovery model in one
/// call (architecture §3.5/§5, spec §4/§7/§8/§9).
///
/// Walks the basin-first hive ([`walk_layout`]), reads each basin's present
/// `gridded_static/<label>.tif` (COG) and `gridded_dynamic/<label>.zarr` (Zarr)
/// artifacts, and reads `outlines.geoparquet` once (when present). Assembles a
/// [`GriddedDiscovery`]: the per-grid representative geometries, the homogeneous
/// gridded field catalog, the distinct delineation labels, and the per-basin observed
/// facts (the G2 precondition + the MED-5 Zarr path).
///
/// **Surfaces gaps as facts, never a verdict.** A basin with no gridded subtree yields
/// an empty [`BasinGridded`]; an absent `outlines.geoparquet` yields an empty
/// delineation list (L1 / G2 enforcement is MS6). Only structural failures
/// (unreadable directory, undecodable present artifact) propagate as errors.
///
/// # Errors
///
/// | Condition | Error |
/// |---|---|
/// | the dataset `path` is not a readable directory | [`CoreError::LayoutWalk`] |
/// | a present gridded subtree cannot be enumerated | [`CoreError::LayoutWalk`] |
/// | a present COG / Zarr artifact's metadata cannot be decoded | [`CoreError::CogRead`] / [`CoreError::ZarrRead`] / [`CoreError::MissingGridGeoref`] / [`CoreError::MissingGriddedCoordinate`] / [`CoreError::UnknownDtype`] |
/// | a present `outlines.geoparquet` cannot be read or is missing a required column | [`CoreError::GeoparquetRead`] / [`CoreError::MissingGeometryColumn`] |
#[instrument(fields(path = %path.as_ref().display()))]
pub fn discover_gridded(path: impl AsRef<Path>) -> Result<GriddedDiscovery, CoreError> {
    let layout: LayoutModel = walk_layout(path)?;

    // Per-basin gridded facts (the G2 precondition + MED-5 path; gaps-as-facts).
    let mut per_basin: Vec<BasinGridded> = Vec::with_capacity(layout.basins().len());
    for basin in layout.basins() {
        per_basin.push(discover_basin_gridded(basin)?);
    }

    // Representative per-grid geometries: every observed artifact's GridInfo, in
    // basin → static → dynamic order (the COG and Zarr GridInfo for a shared label
    // sit side by side so MS6 can compare them — the G2 precondition).
    let grids: Vec<GridInfo> = per_basin
        .iter()
        .flat_map(|b| {
            b.static_artifacts()
                .iter()
                .map(|a| a.grid_info().clone())
                .chain(b.dynamic_artifacts().iter().map(|a| a.grid_info().clone()))
        })
        .collect();

    // The homogeneous gridded field catalog (representative one-basin read, spec §5).
    let gridded_fields = assemble_gridded_field_catalog(&layout, &per_basin)?;

    // The outlines geometry, read once at the root when present (gaps-as-facts). The
    // full `OutlinesInfo` is now retained on the model (additively) so MS6's Geo1 /
    // I1-outlines / M5-outlines legs can read its column-presence + CRS facts.
    let outlines: Option<OutlinesInfo> = if layout.outlines().is_present() {
        Some(read_outlines(layout.outlines().path())?)
    } else {
        debug!("outlines.geoparquet absent; recorded as a gap-as-fact (no outlines info)");
        None
    };
    let delineations: Vec<DelineationLabel> = outlines
        .as_ref()
        .map(|o| o.delineations().to_vec())
        .unwrap_or_default();

    info!(
        basins = per_basin.len(),
        grids = grids.len(),
        gridded_fields = gridded_fields.len(),
        delineations = delineations.len(),
        outlines = outlines.is_some(),
        "assembled the gridded/geometry half of the discovery model"
    );

    Ok(GriddedDiscovery {
        grids,
        gridded_fields,
        delineations,
        per_basin,
        outlines,
    })
}

/// The **combined** shared discovery model both verbs consume (architecture §3.5).
///
/// Pairs the scalar half ([`ScalarDiscovery`](crate::discovery::ScalarDiscovery), MS3)
/// with the gridded / geometry half ([`GriddedDiscovery`], MS4) **without reshaping
/// either** — MS5 (`describe`) *reports* it and MS6 (`validate`) *checks rules over
/// it*. The unified view (architecture §3.5: `basins`, `fields = scalar ⊕ gridded`,
/// `grids`, `delineations`) is exposed by accessors that **borrow** through to the two
/// sub-models, so the underlying types are never copied or restructured.
///
/// Inert / agnostic (spec §1): it composes two fact-only sub-models and adds nothing.
#[derive(Debug, Clone, PartialEq)]
pub struct Discovery {
    scalar: ScalarDiscovery,
    gridded: GriddedDiscovery,
}

impl Discovery {
    /// Pairs an already-built scalar half with an already-built gridded half.
    ///
    /// Neither sub-model is reshaped; [`Discovery`] only *composes* them.
    pub fn new(scalar: ScalarDiscovery, gridded: GriddedDiscovery) -> Self {
        Self { scalar, gridded }
    }

    /// Borrows the scalar half **unchanged** (the four MS3 accessors are reachable
    /// through it: `basins` / `scalar_fields` / `per_basin` / `root_rollups`).
    pub fn scalar(&self) -> &ScalarDiscovery {
        &self.scalar
    }

    /// Borrows the gridded / geometry half (the MS4 accessors: `grids` /
    /// `gridded_fields` / `delineations` / `per_basin`).
    pub fn gridded(&self) -> &GriddedDiscovery {
        &self.gridded
    }

    /// Borrows the discovered basin list (the scalar half's, in stable sorted order).
    ///
    /// The basin list is the scalar half's (folder ids from the walk); the gridded
    /// half's per-basin facts are keyed by the same folder ids.
    pub fn basins(&self) -> &[BasinId] {
        self.scalar.basins()
    }

    /// Returns the unified field catalog `fields = scalar ⊕ gridded` (architecture
    /// §3.5) — the scalar fields followed by the gridded fields, with **no reshaping**
    /// of either half (they are concatenated, not merged or de-duplicated).
    pub fn fields(&self) -> Vec<&Field> {
        self.scalar
            .scalar_fields()
            .iter()
            .chain(self.gridded.gridded_fields().iter())
            .collect()
    }

    /// Borrows the per-grid representative geometries (the gridded half's `grids`).
    pub fn grids(&self) -> &[GridInfo] {
        self.gridded.grids()
    }

    /// Borrows the distinct delineation labels (the gridded half's `delineations`).
    pub fn delineations(&self) -> &[DelineationLabel] {
        self.gridded.delineations()
    }

    /// Borrows the discovered `outlines.geoparquet` facts (the gridded half's
    /// `outlines`), or `None` when the outlines rollup is absent.
    pub fn outlines(&self) -> Option<&OutlinesInfo> {
        self.gridded.outlines()
    }
}

/// Discovers the **complete** shared discovery model in one call — both halves
/// (architecture §3.5/§5, spec §4/§5/§7/§8/§9).
///
/// Runs [`discover_scalar`](crate::discovery::discover_scalar) (MS3) and
/// [`discover_gridded`] over the same dataset `path` and pairs them in a [`Discovery`]
/// without reshaping either. This is the single model MS5 (`describe`) and MS6
/// (`validate`) consume.
///
/// **Surfaces gaps as facts, never a verdict** (the discipline of both halves).
///
/// # Errors
///
/// Propagates any error from either half (see
/// [`discover_scalar`](crate::discovery::discover_scalar) and [`discover_gridded`]).
#[instrument(fields(path = %path.as_ref().display()))]
pub fn discover(path: impl AsRef<Path>) -> Result<Discovery, CoreError> {
    let path = path.as_ref();
    let scalar = discover_scalar(path)?;
    let gridded = discover_gridded(path)?;
    info!("assembled the complete shared discovery model (scalar ⊕ gridded)");
    Ok(Discovery::new(scalar, gridded))
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use crate::cog_reader::CogGrid;
    use crate::discovery::ScalarDiscovery;
    use crate::field::Quadrant;
    use crate::grid::GridInfo;
    use crate::gridded_discovery::{Discovery, GriddedDiscovery, discover, discover_gridded};
    use crate::newtypes::{BasinId, Crs, DelineationLabel, GridLabel};
    use crate::zarr_reader::ConsolidatedMetadataSource;

    /// Resolves a path under the committed `conformance/` fixture tree.
    fn conformance(rel: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../conformance")
            .join(rel)
    }

    /// Copies `src` directory recursively into `dst` (test helper).
    fn copy_dir_all(src: &Path, dst: &Path) {
        std::fs::create_dir_all(dst).expect("create temp dir");
        for entry in std::fs::read_dir(src).expect("read_dir") {
            let entry = entry.expect("dir entry");
            let from = entry.path();
            let to = dst.join(entry.file_name());
            if from.is_dir() {
                copy_dir_all(&from, &to);
            } else {
                std::fs::copy(&from, &to).expect("copy file");
            }
        }
    }

    /// Copies the valid fixture into a fresh temp dir, returning its path.
    fn copy_fixture_to_temp(tag: &str) -> PathBuf {
        let dst = std::env::temp_dir().join(format!(
            "hdx-gridded-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        copy_dir_all(&conformance("valid/minimal"), &dst);
        dst
    }

    // --- G2 precondition observed (the prior HIGH defect's end-to-end fix) -------

    #[test]
    fn g2_precondition_observed_cog_and_zarr_extents_coincide_at_10_50() {
        // G2 PRECONDITION (observed, NOT enforced — enforcement is MS6): for basin
        // 0001 the COG `<label>` and the Zarr `<label>` are BOTH `era5` (a shared
        // label across the two subtrees), AND the COG GridInfo.extent equals the Zarr
        // GridInfo.extent. This passes because both readers build edge-based extents:
        // two genuinely-aligned artifacts look aligned (the structural-misread defect
        // is gone). MS4 records this fact; it renders no verdict.
        let gridded: GriddedDiscovery = discover_gridded(conformance("valid/minimal"))
            .expect("the valid fixture must discover the gridded half");

        let basin0001 = gridded
            .per_basin()
            .iter()
            .find(|b| b.basin_id_folder().as_str() == "0001")
            .expect("basin 0001 present");

        // Shared label across the two subtrees.
        let static_labels = basin0001.static_grid_labels();
        let dynamic_labels = basin0001.dynamic_grid_labels();
        assert_eq!(
            static_labels,
            vec![&GridLabel::new("era5")],
            "COG label era5"
        );
        assert_eq!(
            dynamic_labels,
            vec![&GridLabel::new("era5")],
            "Zarr label era5"
        );

        let cog_info: &GridInfo = basin0001.static_artifacts()[0].grid_info();
        let zarr_info: &GridInfo = basin0001.dynamic_artifacts()[0].grid_info();

        // The two extents COINCIDE — byte-true at 10.0 / 50.0 / 11.5 / 48.0.
        assert_eq!(
            cog_info.extent(),
            zarr_info.extent(),
            "COG and Zarr extents coincide (the G2 precondition)"
        );
        assert_eq!(cog_info.extent().west(), 10.0);
        assert_eq!(cog_info.extent().north(), 50.0);
        assert_eq!(cog_info.extent().east(), 11.5);
        assert_eq!(cog_info.extent().south(), 48.0);

        // Resolution + dims coincide too.
        assert_eq!(cog_info.resolution(), zarr_info.resolution());
        assert_eq!(cog_info.width(), zarr_info.width());
        assert_eq!(cog_info.height(), zarr_info.height());
        assert_eq!(cog_info.width(), 6);
        assert_eq!(cog_info.height(), 8);
    }

    // --- Gridded field catalog (G1 self-naming, no name magic) -------------------

    #[test]
    fn gridded_field_catalog_is_exactly_the_three_ordinary_fields() {
        let gridded = discover_gridded(conformance("valid/minimal"))
            .expect("the valid fixture must discover");

        // Exactly {elevation: GriddedStatic, era5_precipitation: GriddedDynamic,
        // era5_precipitation_was_filled: GriddedDynamic}, all grid_label == era5,
        // names verbatim (no {source}_{variable} / companion-mask magic — §2).
        let fields = gridded.gridded_fields();
        assert_eq!(fields.len(), 3, "exactly three gridded fields");

        let elevation = &fields[0];
        assert_eq!(elevation.name().as_str(), "elevation");
        assert_eq!(elevation.quadrant(), Quadrant::GriddedStatic);
        assert_eq!(elevation.grid_label(), Some(&GridLabel::new("era5")));

        let precip = &fields[1];
        assert_eq!(precip.name().as_str(), "era5_precipitation");
        assert_eq!(precip.quadrant(), Quadrant::GriddedDynamic);
        assert_eq!(precip.grid_label(), Some(&GridLabel::new("era5")));

        let mask = &fields[2];
        assert_eq!(mask.name().as_str(), "era5_precipitation_was_filled");
        assert_eq!(mask.quadrant(), Quadrant::GriddedDynamic);
        assert_eq!(mask.grid_label(), Some(&GridLabel::new("era5")));

        // Every gridded field carries grid_label == era5.
        for field in fields {
            assert_eq!(field.grid_label(), Some(&GridLabel::new("era5")));
        }
    }

    // --- G3: every grid CRS is EPSG:4326 -----------------------------------------

    #[test]
    fn g3_every_grid_records_epsg_4326() {
        let gridded = discover_gridded(conformance("valid/minimal"))
            .expect("the valid fixture must discover");

        assert!(!gridded.grids().is_empty(), "grids recorded");
        for grid in gridded.grids() {
            // G3: CF (grid_mapping) and GeoTIFF georef both resolve EPSG:4326.
            assert_eq!(
                grid.crs(),
                &Crs::new("EPSG:4326"),
                "every grid CRS EPSG:4326"
            );
        }
    }

    // --- Geo1 + delineations -----------------------------------------------------

    #[test]
    fn geo1_delineations_are_grit_and_merit() {
        let gridded = discover_gridded(conformance("valid/minimal"))
            .expect("the valid fixture must discover");

        // delineations == {grit, merit} (order-insensitive).
        let mut labels: Vec<&str> = gridded
            .delineations()
            .iter()
            .map(DelineationLabel::as_str)
            .collect();
        labels.sort_unstable();
        assert_eq!(
            labels,
            vec!["grit", "merit"],
            "delineations == {{grit, merit}}"
        );
    }

    // --- MED-5 surfaced at the assembler level -----------------------------------

    #[test]
    fn med5_zarr_path_recorded_as_consolidated_at_the_assembler_level() {
        let gridded = discover_gridded(conformance("valid/minimal"))
            .expect("the valid fixture must discover");

        // The combined model records which Zarr path was taken (consolidated/live or
        // an R3 skip) so MS5/MS6 can report it honestly.
        for basin in gridded.per_basin() {
            for dynamic in basin.dynamic_artifacts() {
                match dynamic.consolidated_source() {
                    ConsolidatedMetadataSource::Consolidated { members } => {
                        assert_eq!(members.len(), 6, "all six members from one read");
                    }
                    ConsolidatedMetadataSource::R3Skip { reason } => {
                        panic!("expected the live consolidated path, got R3 skip: {reason}")
                    }
                }
            }
        }
    }

    // --- Combined model: scalar half unchanged + gridded half alongside ----------

    #[test]
    fn discover_pairs_both_halves_without_reshaping_the_scalar_half() {
        let discovery: Discovery =
            discover(conformance("valid/minimal")).expect("the valid fixture must discover both");

        // SEAM TEST: the scalar half is exposed UNCHANGED — the four MS3 accessors
        // still pass through `Discovery::scalar()`.
        let scalar: &ScalarDiscovery = discovery.scalar();
        let scalar_ids: Vec<&str> = scalar.basins().iter().map(BasinId::as_str).collect();
        assert_eq!(scalar_ids, vec!["0001", "0002", "0003"]);
        assert_eq!(
            scalar.scalar_fields().len(),
            2,
            "two scalar fields unchanged"
        );
        assert_eq!(scalar.per_basin().len(), 3, "three per-basin scalar facts");
        assert!(scalar.root_rollups().outlines_present());

        // The gridded half sits ALONGSIDE it.
        let gridded: &GriddedDiscovery = discovery.gridded();
        assert_eq!(gridded.gridded_fields().len(), 3, "three gridded fields");

        // The unified view: basins (scalar half) + fields = scalar ⊕ gridded.
        assert_eq!(discovery.basins().len(), 3);
        let unified = discovery.fields();
        assert_eq!(
            unified.len(),
            5,
            "fields = 2 scalar ⊕ 3 gridded, concatenated without reshaping"
        );
        // The scalar fields come first, the gridded fields after (no merge).
        assert_eq!(unified[0].name().as_str(), "drainage_area");
        assert_eq!(unified[1].name().as_str(), "streamflow");
        assert_eq!(unified[2].name().as_str(), "elevation");
        assert_eq!(unified[3].name().as_str(), "era5_precipitation");
        assert_eq!(unified[4].name().as_str(), "era5_precipitation_was_filled");

        // Grids + delineations reachable through the combined model.
        assert!(!discovery.grids().is_empty());
        assert_eq!(discovery.delineations().len(), 2);
    }

    /// Compile-level seam check: both halves' accessors are pinned so MS5 builds on a
    /// stable shape and cannot silently reshape either half.
    #[test]
    fn discovery_pins_both_halves_accessors() {
        let discovery = discover(conformance("valid/minimal")).expect("must discover");

        // Scalar-half accessors (through `scalar()`), pinned by type.
        let scalar = discovery.scalar();
        let _b: &[BasinId] = scalar.basins();
        let _sf: &[crate::field::Field] = scalar.scalar_fields();
        let _pb: &[crate::discovery::BasinScalar] = scalar.per_basin();
        let _rr = scalar.root_rollups();

        // Gridded-half accessors (through `gridded()`), pinned by type.
        let gridded = discovery.gridded();
        let _g: &[GridInfo] = gridded.grids();
        let _gf: &[crate::field::Field] = gridded.gridded_fields();
        let _d: &[DelineationLabel] = gridded.delineations();
        let _gpb: &[crate::gridded_discovery::BasinGridded] = gridded.per_basin();

        // Combined-view accessors, pinned by type.
        let _basins: &[BasinId] = discovery.basins();
        let _fields: Vec<&crate::field::Field> = discovery.fields();
        let _grids: &[GridInfo] = discovery.grids();
        let _del: &[DelineationLabel] = discovery.delineations();
    }

    // --- Gaps-as-facts: a basin lacking a gridded subtree ------------------------

    #[test]
    fn basin_without_a_gridded_subtree_discovers_with_empty_gridded_facts() {
        // Gaps-as-facts: a tree where a basin lacks a gridded subtree discovers
        // SUCCESSFULLY with empty gridded facts for that basin (no verdict; L2 is
        // MS6). Build it from the valid fixture by deleting basin 0003's gridded
        // subtrees in a temp copy.
        let temp = copy_fixture_to_temp("nogridded");
        let basin0003 = temp.join("basin=0003");
        std::fs::remove_dir_all(basin0003.join("gridded_static")).expect("rm gridded_static");
        std::fs::remove_dir_all(basin0003.join("gridded_dynamic")).expect("rm gridded_dynamic");

        let result = discover_gridded(&temp);
        std::fs::remove_dir_all(&temp).ok();
        let gridded = result.expect("discovery SUCCEEDS with the gap recorded (no verdict)");

        let basin0003 = gridded
            .per_basin()
            .iter()
            .find(|b| b.basin_id_folder().as_str() == "0003")
            .expect("basin 0003 still enumerated");
        assert!(
            basin0003.static_artifacts().is_empty(),
            "the absent gridded_static subtree records empty facts"
        );
        assert!(
            basin0003.dynamic_artifacts().is_empty(),
            "the absent gridded_dynamic subtree records empty facts"
        );

        // The other basins still discover their gridded facts; the field catalog +
        // delineations are still populated from the present basins.
        assert_eq!(
            gridded.gridded_fields().len(),
            3,
            "catalog from present basins"
        );
        assert_eq!(gridded.delineations().len(), 2, "delineations still read");
    }

    // --- Combined re-read consistency: the COG field comes back identical --------

    #[test]
    fn catalog_band_field_matches_the_per_artifact_read() {
        // The catalog re-reads the representative COG for its band field; assert that
        // re-read agrees with a direct read (no reshaping of the band).
        let gridded = discover_gridded(conformance("valid/minimal"))
            .expect("the valid fixture must discover");
        let elevation = &gridded.gridded_fields()[0];

        let direct: CogGrid = crate::cog_reader::read_cog_grid(
            conformance("valid/minimal/basin=0001/gridded_static/era5.tif"),
            GridLabel::new("era5"),
        )
        .expect("direct COG read");
        assert_eq!(
            elevation,
            direct.field(),
            "catalog band == direct read band"
        );
    }
}
