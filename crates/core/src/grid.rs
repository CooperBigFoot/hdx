//! Shared gridded-geometry value types — the typed vocabulary the gridded /
//! geometry readers and the discovery assembler consume (spec §7/§8,
//! architecture §3.5).
//!
//! This module is **pure types + one convention**. It performs **no IO**.
//!
//! ## The single grid convention (the load-bearing decision)
//!
//! HDX records a grid's geometry as a **north-west cell-EDGE origin** plus a
//! per-axis signed [`GridResolution`] — the GeoTIFF-native form. Both gridded
//! readers build the *same* [`GridExtent`] this way:
//!
//! - The **COG reader** takes the affine tiepoint verbatim: GeoTIFF
//!   `ModelTiepoint` is already a cell-edge origin, so no conversion is needed.
//! - The **Zarr reader** stores cell-**center** coordinate arrays (CF
//!   convention), so it converts the first center to an edge with the half-pixel
//!   rule via [`center_to_edge`]: `west = lon[0] − x_res/2`, and
//!   `north = lat[0] − y_res/2` (where `y_res < 0` for a north-up raster, so the
//!   north edge sits *above* the first center). Signs are per axis.
//!
//! For two genuinely-aligned artifacts both yield **west = 10.0 / north = 50.0**
//! (Zarr centers `lon[0]=10.125`, `lat[0]=49.875`, `res=0.25`; COG tiepoint edges
//! `10.0`/`50.0`). With this single convention two aligned artifacts yield
//! *identical* extents, so the §8 shared-label alignment precondition is observable.
//!
//! ## A gridded field is an ordinary [`Field`] (no new type)
//!
//! HDX is **inert and agnostic** (spec §1/§2): a gridded field is just an ordinary
//! [`Field`] with a [`Quadrant::GriddedStatic`] / [`Quadrant::GriddedDynamic`]
//! quadrant and `Some(GridLabel)`. This module adds **no** new field type — the
//! readers construct plain [`Field`]s. A name like `era5_precipitation_was_filled`
//! carries no magic: it is recorded verbatim, with no `{source}_{variable}` split
//! and no companion-mask special-casing.
//!
//! ## No pixel / no chunk
//!
//! No type here holds a pixel buffer or a chunk payload, and the readers layered on
//! these types read **metadata only** (architecture §1): the Zarr reader reads
//! `zarr.json` + the 1-D `lat`/`lon`/`time` coordinate arrays + CF `grid_mapping`,
//! and the COG reader reads GeoTIFF tags + band metadata + georef — **never** a
//! `c/` chunk payload or a pixel raster. The `gridded_*` subtrees are opaque leaves
//! to the layout walk and metadata-only to the readers.
//!
//! ## Inert / agnostic (spec §1)
//!
//! Every datum here is a structural fact: a signed resolution, four edge
//! coordinates, a width/height, a [`GridLabel`], a recorded [`Crs`]. No type or
//! field carries transform, role, semantic type, or provenance.
//!
//! Glossary:
//!
//! | Term | Meaning |
//! |---|---|
//! | cell-edge origin | the NW corner of the NW cell (GeoTIFF-native), *not* the cell center |
//! | cell center | the coordinate at the middle of a cell (CF / Zarr `lat`/`lon` arrays) |
//! | half-pixel rule | center→edge conversion: shift the first center out by `res/2` |
//! | signed resolution | per-axis step; `x_res > 0` marches east, `y_res < 0` marches south |
//! | grid label | the grid *family* a gridded field lives on (spec §8) |

use tracing::{debug, instrument};

use crate::newtypes::{Crs, GridLabel};

/// Per-axis signed grid resolution in CRS units (spec §7.2, architecture §3.5).
///
/// The two axes carry their **signs**, matching a north-up raster's affine: a
/// positive `x_res` marches east as the column index grows, and a *negative*
/// `y_res` marches south as the row index grows (the GeoTIFF `ModelPixelScale` /
/// affine convention). HDX records the resolution and interprets none of it — there
/// is no canonical unit and no reprojection.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GridResolution {
    x_res: f64,
    y_res: f64,
}

impl GridResolution {
    /// Constructs a resolution from its two signed per-axis steps (no parsing).
    ///
    /// The caller supplies the signs: `x_res > 0` for an east-marching column axis,
    /// `y_res < 0` for a south-marching (north-up) row axis. HDX does not coerce or
    /// validate the signs — it records what the file declares.
    pub fn new(x_res: f64, y_res: f64) -> Self {
        Self { x_res, y_res }
    }

    pub fn x_res(&self) -> f64 {
        self.x_res
    }

    pub fn y_res(&self) -> f64 {
        self.y_res
    }
}

/// A grid's extent as a **north-west cell-EDGE origin** plus its far edges
/// (spec §7.2, architecture §3.5).
///
/// `west`/`north` are the coordinates of the **outer edge of the north-west cell**
/// — *not* a cell center — matching the GeoTIFF affine tiepoint. `east`/`south` are
/// the opposite far edges, derived from the origin, the signed resolution, and the
/// pixel dimensions. This is the **single grid convention** every reader builds:
///
/// - The COG reader supplies the tiepoint verbatim (already edge-based).
/// - The Zarr reader first converts its cell-center coordinate arrays to edges
///   with the half-pixel rule (see [`center_to_edge`] and the module docs):
///   `west = lon[0] − x_res/2`, `north = lat[0] − y_res/2` (signs per axis;
///   `y_res < 0` puts the north edge above the first center).
///
/// Build it through [`GridExtent::from_edge_origin`] so both readers derive
/// `east`/`south` identically; for a 6×8 / 0.25° grid this yields `west = 10.0`,
/// `north = 50.0`, `east = 11.5`, `south = 48.0`.
///
/// Inert / agnostic (spec §1): four bare edge coordinates, no transform/role/semantic.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GridExtent {
    west: f64,
    north: f64,
    east: f64,
    south: f64,
}

impl GridExtent {
    /// Builds the extent from the NW cell-edge origin, the resolution, and the
    /// pixel dimensions, deriving the far (`east`/`south`) edges.
    ///
    /// `west`/`north` are the NW cell-edge origin (the GeoTIFF tiepoint convention).
    /// `res` is the cell size as a **positive magnitude** (the common
    /// `ModelPixelScale` value); the far edges march `width` cells east and `height`
    /// cells south:
    ///
    /// - `east = west + res * width`
    /// - `south = north − res * height`
    ///
    /// Both readers call this with the same edge-origin convention, so two
    /// genuinely-aligned artifacts produce identical extents. For example,
    /// `from_edge_origin(10.0, 50.0, 0.25, 6, 8)` yields `east = 11.5`,
    /// `south = 48.0`, byte-matching the decoded COG bounds.
    pub fn from_edge_origin(west: f64, north: f64, res: f64, width: usize, height: usize) -> Self {
        let east = west + res * width as f64;
        let south = north - res * height as f64;
        debug!(
            west,
            north, east, south, "derived grid extent from edge origin"
        );
        Self {
            west,
            north,
            east,
            south,
        }
    }

    pub fn west(&self) -> f64 {
        self.west
    }

    pub fn north(&self) -> f64 {
        self.north
    }

    pub fn east(&self) -> f64 {
        self.east
    }

    pub fn south(&self) -> f64 {
        self.south
    }
}

/// The representative geometry of a single grid *label* (architecture §3.5, spec §7).
///
/// HDX records, per grid label, one representative [`GridExtent`] +
/// [`GridResolution`] + pixel dimensions + the recorded [`Crs`]. A shared label
/// across the `gridded_static` (COG) and `gridded_dynamic` (Zarr) subtrees signals
/// cell-for-cell alignment (spec §8): the two labels' extents coincide (the §14 G2
/// precondition).
///
/// The `crs` is recorded per the CRS-recording rule: an `EPSG:<code>` string
/// whenever an EPSG authority/code resolves, else the raw CRS string verbatim. HDX
/// records the CRS and compares nothing here.
///
/// Inert / agnostic (spec §1): geometry + a CRS string, no transform/role/semantic.
#[derive(Debug, Clone, PartialEq)]
pub struct GridInfo {
    grid_label: GridLabel,
    extent: GridExtent,
    resolution: GridResolution,
    width: usize,
    height: usize,
    crs: Crs,
}

impl GridInfo {
    /// Constructs the per-grid-label representative geometry (no parsing).
    ///
    /// The readers supply each field already in the single edge convention: an
    /// `extent` built via [`GridExtent::from_edge_origin`], a signed `resolution`,
    /// the pixel `width`/`height`, and the recorded `crs`.
    #[instrument(skip(extent, resolution, crs))]
    pub fn new(
        grid_label: GridLabel,
        extent: GridExtent,
        resolution: GridResolution,
        width: usize,
        height: usize,
        crs: Crs,
    ) -> Self {
        debug!(
            grid_label = grid_label.as_str(),
            width,
            height,
            crs = crs.as_str(),
            "constructed grid info"
        );
        Self {
            grid_label,
            extent,
            resolution,
            width,
            height,
            crs,
        }
    }

    pub fn grid_label(&self) -> &GridLabel {
        &self.grid_label
    }

    pub fn extent(&self) -> GridExtent {
        self.extent
    }

    pub fn resolution(&self) -> GridResolution {
        self.resolution
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    pub fn crs(&self) -> &Crs {
        &self.crs
    }
}

/// Converts a coordinate axis's **first cell center** to its outer **cell edge**
/// by the half-pixel rule.
///
/// CF / Zarr coordinate arrays store cell *centers*; the single grid convention
/// records cell *edges*. The outer edge sits half a cell *outside* the first
/// center, in the direction the axis marches: `edge = first_center − res/2`, where
/// `res` is the **signed** per-axis resolution.
///
/// - x-axis (`x_res > 0`, east-marching): `west = lon[0] − x_res/2`, shifting the
///   edge west of the first center (e.g. `10.125 − 0.25/2 = 10.0`).
/// - y-axis (`y_res < 0`, south-marching): `north = lat[0] − y_res/2`, and since
///   `y_res` is negative this *adds* half a pixel, shifting the edge north of the
///   first center (e.g. `49.875 − (−0.25)/2 = 49.875 + 0.125 = 50.0`).
///
/// Passing the signed `res` makes the per-axis sign handling automatic — the same
/// formula serves both axes.
pub fn center_to_edge(first_center: f64, res: f64) -> f64 {
    first_center - res / 2.0
}

#[cfg(test)]
mod tests {
    use crate::field::{Dtype, Field, Quadrant, Units};
    use crate::grid::{GridExtent, GridInfo, GridResolution, center_to_edge};
    use crate::newtypes::{Crs, FieldName, GridLabel};

    #[test]
    fn center_to_edge_pins_the_half_pixel_rule() {
        // x-axis: the first lon center 10.125 with res +0.25 → west edge 10.0.
        assert_eq!(center_to_edge(10.125, 0.25), 10.0);

        // y-axis: the first lat center 49.875 with signed res −0.25 → north edge
        // 50.0 (the negative sign adds the half pixel, pushing the edge north).
        assert_eq!(center_to_edge(49.875, -0.25), 50.0);
    }

    #[test]
    fn from_edge_origin_derives_far_edges_matching_cog_bounds() {
        // A 6×8 / 0.25° grid: NW edge origin 10.0/50.0.
        let extent = GridExtent::from_edge_origin(10.0, 50.0, 0.25, 6, 8);
        assert_eq!(extent.west(), 10.0);
        assert_eq!(extent.north(), 50.0);
        assert_eq!(extent.east(), 11.5);
        assert_eq!(extent.south(), 48.0);
    }

    #[test]
    fn grid_info_constructs_with_epsg_crs() {
        let extent = GridExtent::from_edge_origin(10.0, 50.0, 0.25, 6, 8);
        let resolution = GridResolution::new(0.25, -0.25);
        let info = GridInfo::new(
            GridLabel::new("era5"),
            extent,
            resolution,
            6,
            8,
            Crs::new("EPSG:4326"),
        );

        assert_eq!(info.grid_label(), &GridLabel::new("era5"));
        assert_eq!(info.crs(), &Crs::new("EPSG:4326"));
        assert_eq!(info.width(), 6);
        assert_eq!(info.height(), 8);
        assert_eq!(info.resolution().x_res(), 0.25);
        assert_eq!(info.resolution().y_res(), -0.25);
        assert_eq!(info.extent(), extent);
    }

    #[test]
    fn gridded_field_is_an_ordinary_field_with_no_magic() {
        // A gridded field is just a Field: gridded quadrant + Some(GridLabel).
        // A `{source}_{variable}` companion-mask name carries no special handling —
        // it is recorded verbatim (spec §2).
        let field = Field::new(
            FieldName::new("era5_precipitation_was_filled"),
            Quadrant::GriddedDynamic,
            Dtype::Bool,
            Units::none(),
            None,
            Some(GridLabel::new("era5")),
        )
        .expect("gridded + Some(label) must construct");

        assert_eq!(field.name().as_str(), "era5_precipitation_was_filled");
        assert_eq!(field.quadrant(), Quadrant::GriddedDynamic);
        assert_eq!(field.grid_label(), Some(&GridLabel::new("era5")));
    }
}
