//! The field model — HDX's spine, encoded as the 2×2 quadrant (spec §2).
//!
//! The unit of HDX is the **field**, classified on two independent axes:
//! [`Temporal`] (`static` vs `dynamic`) and [`Shape`] (`scalar` vs `gridded`).
//! Their product is the four [`Quadrant`]s. A field also carries a name, a
//! [`Dtype`], optional [`Units`], and — *iff* it is gridded — a [`GridLabel`].
//!
//! HDX is **inert and agnostic** (spec §1): a [`Field`] carries only `name`,
//! `quadrant`, `dtype`, `units`, and `grid_label`. There is no transform, role,
//! semantic type, or provenance. [`Dtype`] is a *closed* enum whose variants are
//! opaque to semantics — HDX records the physical encoding and interprets none of
//! it. [`Units`] is an unparsed optional producer string.
//!
//! The two axes are **enums, never booleans** (architecture §3.3): a `bool` says
//! nothing about which axis or which pole it names, whereas `Temporal::Dynamic`
//! is self-documenting. [`Field::new`] is the boundary: it enforces the structural
//! invariant `grid_label.is_some()` ⇔ `Shape::Gridded`, so the two illegal combos
//! (a gridded field with no label, a scalar field with a label) are
//! unrepresentable past construction.
//!
//! Glossary:
//!
//! | Term | Meaning |
//! |---|---|
//! | temporal | the time axis: one value (`static`) vs a series (`dynamic`) |
//! | shape | the space axis: a single value (`scalar`) vs a per-cell field (`gridded`) |
//! | quadrant | the (temporal × shape) classification of a single field |
//! | dtype | the physical element encoding (`f32`, `i64`, …); opaque to semantics |
//! | grid label | the grid family a gridded field lives on (spec §8) |

use tracing::{debug, instrument, warn};

use crate::error::CoreError;
use crate::newtypes::{FieldName, GridLabel};

/// The temporal axis of a field (spec §2): a single value or a time series.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Temporal {
    /// One value, with no time axis (per-basin shape `[]` or `[Y,X]`).
    Static,
    /// A time series (per-basin shape `[T]` or `[T,Y,X]`).
    Dynamic,
}

/// The spatial-shape axis of a field (spec §2): a single value or a per-cell field.
///
/// The poles are `scalar` vs `gridded`, deliberately **not** "lumped vs gridded":
/// "lumped" smuggles in a reduction, whereas a scalar value (e.g. outlet
/// streamflow) is often scalar by nature. HDX cares only about data shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// A single value per basin (per-basin shape `[]` or `[T]`).
    Scalar,
    /// A per-cell field over the basin bbox (per-basin shape `[Y,X]` or `[T,Y,X]`).
    Gridded,
}

/// The four quadrants of the field model — the product of [`Temporal`] × [`Shape`]
/// (spec §2).
///
/// The quadrant is a **per-field** classification, never a dataset-level mode: a
/// single dataset's schema may mix all four freely. Use [`Quadrant::from_axes`] to
/// build a quadrant from its two axes, and [`Quadrant::temporal`] /
/// [`Quadrant::shape`] to recover them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quadrant {
    /// `scalar · static` — per-basin shape `[]` (e.g. drainage area).
    ScalarStatic,
    /// `scalar · dynamic` — per-basin shape `[T]` (e.g. outlet streamflow).
    ScalarDynamic,
    /// `gridded · static` — per-basin shape `[Y,X]` (e.g. an elevation raster).
    GriddedStatic,
    /// `gridded · dynamic` — per-basin shape `[T,Y,X]` (e.g. precip over a grid).
    GriddedDynamic,
}

impl Quadrant {
    /// Builds the quadrant from its two independent axes.
    ///
    /// This is total: every (temporal, shape) pair maps to exactly one quadrant,
    /// and [`Quadrant::temporal`] / [`Quadrant::shape`] invert it exactly.
    pub fn from_axes(temporal: Temporal, shape: Shape) -> Self {
        match (shape, temporal) {
            (Shape::Scalar, Temporal::Static) => Quadrant::ScalarStatic,
            (Shape::Scalar, Temporal::Dynamic) => Quadrant::ScalarDynamic,
            (Shape::Gridded, Temporal::Static) => Quadrant::GriddedStatic,
            (Shape::Gridded, Temporal::Dynamic) => Quadrant::GriddedDynamic,
        }
    }

    /// Recovers the temporal axis of this quadrant.
    pub fn temporal(&self) -> Temporal {
        match self {
            Quadrant::ScalarStatic | Quadrant::GriddedStatic => Temporal::Static,
            Quadrant::ScalarDynamic | Quadrant::GriddedDynamic => Temporal::Dynamic,
        }
    }

    /// Recovers the spatial-shape axis of this quadrant.
    pub fn shape(&self) -> Shape {
        match self {
            Quadrant::ScalarStatic | Quadrant::ScalarDynamic => Shape::Scalar,
            Quadrant::GriddedStatic | Quadrant::GriddedDynamic => Shape::Gridded,
        }
    }
}

/// The physical element encoding of a field (architecture §3.3).
///
/// This is a **closed** set — HDX recognizes exactly these encodings and rejects
/// anything else at the boundary via [`parse_dtype`]. The variants are **opaque to
/// semantics**: HDX records *how a value is physically encoded*, never *what it
/// means* (continuous/categorical interpretation is the consumer's job, spec §1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dtype {
    /// 32-bit IEEE-754 floating point.
    F32,
    /// 64-bit IEEE-754 floating point.
    F64,
    /// 32-bit signed integer.
    I32,
    /// 64-bit signed integer.
    I64,
    /// Boolean (e.g. a QC / gap mask, spec §12).
    Bool,
    /// A full timestamp (date + time) — the temporal element type (spec §6).
    Timestamp,
}

impl Dtype {
    /// Returns the canonical string for this dtype (the form round-tripped by
    /// [`parse_dtype`]).
    pub fn as_str(&self) -> &'static str {
        match self {
            Dtype::F32 => "f32",
            Dtype::F64 => "f64",
            Dtype::I32 => "i32",
            Dtype::I64 => "i64",
            Dtype::Bool => "bool",
            Dtype::Timestamp => "timestamp",
        }
    }
}

/// Parses a raw physical-type string into a closed [`Dtype`] at the boundary.
///
/// The accepted strings are the canonical names plus the common aliases that
/// physical formats (Arrow/parquet, NumPy/Zarr, GDAL) emit. Matching is
/// case-sensitive on the documented spellings below; nothing else is coerced.
///
/// | Dtype | Accepted strings |
/// |---|---|
/// | [`Dtype::F32`] | `f32`, `float32` |
/// | [`Dtype::F64`] | `f64`, `float64`, `double` |
/// | [`Dtype::I32`] | `i32`, `int32` |
/// | [`Dtype::I64`] | `i64`, `int64` |
/// | [`Dtype::Bool`] | `bool`, `boolean` |
/// | [`Dtype::Timestamp`] | `timestamp`, `datetime` |
///
/// # Errors
///
/// | Condition | Error |
/// |---|---|
/// | `s` is not one of the documented strings (including the empty string) | [`CoreError::UnknownDtype`] (with `found` echoing `s`) |
///
/// This function never panics: every unrecognized input — including `""` and
/// `"complex128"` — returns `Err(CoreError::UnknownDtype { .. })`.
#[instrument]
pub fn parse_dtype(s: &str) -> Result<Dtype, CoreError> {
    let dtype = match s {
        "f32" | "float32" => Dtype::F32,
        "f64" | "float64" | "double" => Dtype::F64,
        "i32" | "int32" => Dtype::I32,
        "i64" | "int64" => Dtype::I64,
        "bool" | "boolean" => Dtype::Bool,
        "timestamp" | "datetime" => Dtype::Timestamp,
        other => {
            warn!(found = other, "rejecting unknown dtype");
            return Err(CoreError::UnknownDtype {
                found: other.to_string(),
            });
        }
    };
    debug!(input = s, dtype = dtype.as_str(), "parsed dtype");
    Ok(dtype)
}

/// Units for a field — an optional, unparsed producer string (architecture §3.3).
///
/// HDX is inert (spec §1): units are recorded verbatim or absent, and HDX parses
/// none of their contents (no unit algebra, no canonicalization). A field with no
/// declared units carries [`Units::none`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Units(Option<String>);

impl Units {
    /// Constructs units from an optional raw string (no parsing occurs).
    pub fn new(value: Option<impl Into<String>>) -> Self {
        Self(value.map(Into::into))
    }

    /// Constructs the absent ("no units") value.
    pub fn none() -> Self {
        Self(None)
    }

    /// Borrows the underlying units string, if any.
    pub fn as_deref(&self) -> Option<&str> {
        self.0.as_deref()
    }
}

/// A single HDX field — the unit of the contract (spec §2, architecture §3.3).
///
/// A field is `name` + `quadrant` + `dtype` + `units` + (for gridded fields) a
/// `grid_label`. Fields are private; access them via the getters. Construction
/// goes through [`Field::new`], which enforces the structural invariant
/// `grid_label.is_some()` ⇔ `Shape::Gridded`.
///
/// HDX is inert and agnostic (spec §1): a field carries none of transform, role,
/// semantic type, or provenance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    name: FieldName,
    quadrant: Quadrant,
    dtype: Dtype,
    units: Units,
    grid_label: Option<GridLabel>,
}

impl Field {
    /// Constructs a field, enforcing `grid_label.is_some()` ⇔ `Shape::Gridded`.
    ///
    /// The grid label is structurally tied to the shape axis: a gridded field
    /// lives on a named grid family (spec §8), and a scalar field has no grid.
    /// This constructor is the only way to build a [`Field`], so the two illegal
    /// combinations are unrepresentable in any constructed value.
    ///
    /// # Errors
    ///
    /// | Condition | Error |
    /// |---|---|
    /// | `quadrant.shape() == Shape::Gridded` but `grid_label` is `None` | [`CoreError::MismatchedGridLabel`] |
    /// | `quadrant.shape() == Shape::Scalar` but `grid_label` is `Some(..)` | [`CoreError::MismatchedGridLabel`] |
    #[instrument(skip(units, grid_label))]
    pub fn new(
        name: FieldName,
        quadrant: Quadrant,
        dtype: Dtype,
        units: Units,
        grid_label: Option<GridLabel>,
    ) -> Result<Self, CoreError> {
        match (quadrant.shape(), grid_label.is_some()) {
            (Shape::Gridded, true) | (Shape::Scalar, false) => {
                debug!(
                    field = name.as_str(),
                    dtype = dtype.as_str(),
                    "constructed field"
                );
                Ok(Self {
                    name,
                    quadrant,
                    dtype,
                    units,
                    grid_label,
                })
            }
            (shape, has_label) => {
                warn!(
                    field = name.as_str(),
                    has_label, "grid_label does not match field shape"
                );
                Err(CoreError::MismatchedGridLabel {
                    field: name.as_str().to_string(),
                    gridded: shape == Shape::Gridded,
                    has_label,
                })
            }
        }
    }

    /// Borrows the field name.
    pub fn name(&self) -> &FieldName {
        &self.name
    }

    /// Returns the field's quadrant.
    pub fn quadrant(&self) -> Quadrant {
        self.quadrant
    }

    /// Returns the field's dtype.
    pub fn dtype(&self) -> Dtype {
        self.dtype
    }

    /// Borrows the field's units.
    pub fn units(&self) -> &Units {
        &self.units
    }

    /// Borrows the field's grid label, present iff the field is gridded.
    pub fn grid_label(&self) -> Option<&GridLabel> {
        self.grid_label.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use crate::error::CoreError;
    use crate::field::{Dtype, Field, Quadrant, Shape, Temporal, Units, parse_dtype};
    use crate::newtypes::{FieldName, GridLabel};

    #[test]
    fn from_axes_covers_all_four_quadrants() {
        assert_eq!(
            Quadrant::from_axes(Temporal::Static, Shape::Scalar),
            Quadrant::ScalarStatic
        );
        assert_eq!(
            Quadrant::from_axes(Temporal::Dynamic, Shape::Scalar),
            Quadrant::ScalarDynamic
        );
        assert_eq!(
            Quadrant::from_axes(Temporal::Static, Shape::Gridded),
            Quadrant::GriddedStatic
        );
        assert_eq!(
            Quadrant::from_axes(Temporal::Dynamic, Shape::Gridded),
            Quadrant::GriddedDynamic
        );
    }

    #[test]
    fn temporal_and_shape_round_trip_through_quadrant() {
        for temporal in [Temporal::Static, Temporal::Dynamic] {
            for shape in [Shape::Scalar, Shape::Gridded] {
                let quadrant = Quadrant::from_axes(temporal, shape);
                assert_eq!(quadrant.temporal(), temporal);
                assert_eq!(quadrant.shape(), shape);
            }
        }
    }

    #[test]
    fn parse_dtype_maps_every_documented_string() {
        assert_eq!(parse_dtype("f32").expect("f32"), Dtype::F32);
        assert_eq!(parse_dtype("float32").expect("float32"), Dtype::F32);
        assert_eq!(parse_dtype("f64").expect("f64"), Dtype::F64);
        assert_eq!(parse_dtype("float64").expect("float64"), Dtype::F64);
        assert_eq!(parse_dtype("double").expect("double"), Dtype::F64);
        assert_eq!(parse_dtype("i32").expect("i32"), Dtype::I32);
        assert_eq!(parse_dtype("int32").expect("int32"), Dtype::I32);
        assert_eq!(parse_dtype("i64").expect("i64"), Dtype::I64);
        assert_eq!(parse_dtype("int64").expect("int64"), Dtype::I64);
        assert_eq!(parse_dtype("bool").expect("bool"), Dtype::Bool);
        assert_eq!(parse_dtype("boolean").expect("boolean"), Dtype::Bool);
        assert_eq!(
            parse_dtype("timestamp").expect("timestamp"),
            Dtype::Timestamp
        );
        assert_eq!(parse_dtype("datetime").expect("datetime"), Dtype::Timestamp);
    }

    #[test]
    fn parse_dtype_round_trips_canonical_strings() {
        for dtype in [
            Dtype::F32,
            Dtype::F64,
            Dtype::I32,
            Dtype::I64,
            Dtype::Bool,
            Dtype::Timestamp,
        ] {
            assert_eq!(
                parse_dtype(dtype.as_str()).expect("canonical string must re-parse"),
                dtype
            );
        }
    }

    #[test]
    fn parse_dtype_rejects_unknown_without_panicking() {
        // The empty string and an unsupported encoding both error via Result —
        // no panic, with the raw input echoed back.
        for input in ["complex128", "", "Float32", "u8", "string"] {
            match parse_dtype(input) {
                Err(CoreError::UnknownDtype { found }) => {
                    assert_eq!(found, input, "the error must echo the rejected input");
                }
                other => panic!("expected UnknownDtype for {input:?}, got {other:?}"),
            }
        }
    }

    #[test]
    fn field_new_gridded_with_label_is_ok() {
        let field = Field::new(
            FieldName::new("ERA5_precipitation"),
            Quadrant::GriddedDynamic,
            Dtype::F32,
            Units::new(Some("mm")),
            Some(GridLabel::new("era5")),
        )
        .expect("gridded + Some(label) must construct");
        assert_eq!(field.quadrant(), Quadrant::GriddedDynamic);
        assert_eq!(field.grid_label(), Some(&GridLabel::new("era5")));
    }

    #[test]
    fn field_new_gridded_without_label_errors() {
        match Field::new(
            FieldName::new("elevation"),
            Quadrant::GriddedStatic,
            Dtype::F32,
            Units::new(Some("m")),
            None,
        ) {
            Err(CoreError::MismatchedGridLabel {
                field,
                gridded,
                has_label,
            }) => {
                assert_eq!(field, "elevation");
                assert!(gridded);
                assert!(!has_label);
            }
            other => panic!("expected MismatchedGridLabel, got {other:?}"),
        }
    }

    #[test]
    fn field_new_scalar_with_label_errors() {
        match Field::new(
            FieldName::new("drainage_area"),
            Quadrant::ScalarStatic,
            Dtype::F64,
            Units::new(Some("km2")),
            Some(GridLabel::new("era5")),
        ) {
            Err(CoreError::MismatchedGridLabel {
                field,
                gridded,
                has_label,
            }) => {
                assert_eq!(field, "drainage_area");
                assert!(!gridded);
                assert!(has_label);
            }
            other => panic!("expected MismatchedGridLabel, got {other:?}"),
        }
    }

    #[test]
    fn field_new_scalar_without_label_is_ok() {
        let field = Field::new(
            FieldName::new("streamflow"),
            Quadrant::ScalarDynamic,
            Dtype::F64,
            Units::none(),
            None,
        )
        .expect("scalar + None must construct");
        assert_eq!(field.quadrant(), Quadrant::ScalarDynamic);
        assert_eq!(field.grid_label(), None);
        assert_eq!(field.units().as_deref(), None);
    }

    #[test]
    fn units_round_trip_without_parsing() {
        // `Units::new(Some(..))` preserves the string verbatim; no parsing.
        assert_eq!(Units::new(Some("m3/s")).as_deref(), Some("m3/s"));
        assert_eq!(Units::new(Some("")).as_deref(), Some(""));
        assert_eq!(Units::new(None::<String>).as_deref(), None);
        assert_eq!(Units::none().as_deref(), None);
    }
}
