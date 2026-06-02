//! Opaque domain newtypes that wrap producer-chosen strings.
//!
//! Each type wraps a `String` with a private field so a confusion-prone value
//! (an id, a label, a CRS, a cadence) cannot be swapped for another at a call
//! site. HDX is **inert and agnostic** (spec §1): these wrappers carry the value
//! and nothing else — no transform, role, semantic type, or provenance. HDX
//! parses none of their contents; they are opaque producer strings (spec §2/§3/§9).
//!
//! [`BasinId`] and [`GridLabel`] additionally derive `Eq`/`Hash` because they are
//! used as set/map keys during discovery and validation; the other newtypes only
//! need equality comparison.

/// A basin id, unique within a single dataset (spec §3).
///
/// How the id is minted (gauge id, hash, integer, UUID) is the producer's
/// business; HDX only requires uniqueness, checked later. Derives `Eq`/`Hash`
/// so basins can key sets and maps during discovery.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BasinId(String);

impl BasinId {
    /// Wraps a raw id string.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Borrows the underlying id string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A field name — a unique, producer-chosen, opaque string (spec §2).
///
/// HDX assigns the name no meaning: there is no canonical vocabulary and no
/// source/variable split. The column / CF variable / band is named exactly this.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FieldName(String);

impl FieldName {
    /// Wraps a raw field-name string.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Borrows the underlying field-name string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A grid label naming a grid *family* (spec §8).
///
/// The artifact is named after this label; a label shared across the
/// `gridded_static` and `gridded_dynamic` subtrees signals cell-for-cell
/// alignment. Derives `Eq`/`Hash` so the grid-label set can be compared across
/// basins during validation.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GridLabel(String);

impl GridLabel {
    /// Wraps a raw grid-label string.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Borrows the underlying grid-label string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A neutral delineation label (spec §9).
///
/// One row per delineation in `outlines.geoparquet`; the value is a producer
/// label (MERIT, GRIT, HydroBASINS, a custom run, a hand-drawn polygon) and is
/// *not* assumed to name a published hydrofabric. HDX interprets nothing.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DelineationLabel(String);

impl DelineationLabel {
    /// Wraps a raw delineation-label string.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Borrows the underlying delineation-label string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A coordinate reference system, e.g. `"EPSG:4326"` (spec §7/§11).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Crs(String);

impl Crs {
    /// Wraps a raw CRS string.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Borrows the underlying CRS string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A dataset-wide cadence / calendar convention, e.g. `"daily"` (spec §6/§11).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Cadence(String);

impl Cadence {
    /// Wraps a raw cadence string.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Borrows the underlying cadence string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A dataset name — generic dataset identity (spec §11).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DatasetName(String);

impl DatasetName {
    /// Wraps a raw dataset-name string.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Borrows the underlying dataset-name string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The tool/version that wrote the dataset (spec §11).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProducerVersion(String);

impl ProducerVersion {
    /// Wraps a raw producer-version string.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Borrows the underlying producer-version string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::collections::HashSet;

    use crate::newtypes::BasinId;
    use crate::newtypes::Cadence;
    use crate::newtypes::Crs;
    use crate::newtypes::DatasetName;
    use crate::newtypes::DelineationLabel;
    use crate::newtypes::FieldName;
    use crate::newtypes::GridLabel;
    use crate::newtypes::ProducerVersion;

    #[test]
    fn newtypes_round_trip() {
        assert_eq!(BasinId::new("v").as_str(), "v");
        assert_eq!(FieldName::new("v").as_str(), "v");
        assert_eq!(GridLabel::new("v").as_str(), "v");
        assert_eq!(DelineationLabel::new("v").as_str(), "v");
        assert_eq!(Crs::new("v").as_str(), "v");
        assert_eq!(Cadence::new("v").as_str(), "v");
        assert_eq!(DatasetName::new("v").as_str(), "v");
        assert_eq!(ProducerVersion::new("v").as_str(), "v");
    }

    #[test]
    fn newtypes_accept_owned_strings() {
        let owned = String::from("01013500");
        assert_eq!(BasinId::new(owned).as_str(), "01013500");
    }

    #[test]
    fn partial_eq_works() {
        assert_eq!(BasinId::new("a"), BasinId::new("a"));
        assert_ne!(BasinId::new("a"), BasinId::new("b"));
        assert_eq!(Crs::new("EPSG:4326"), Crs::new("EPSG:4326"));
        assert_ne!(Cadence::new("daily"), Cadence::new("hourly"));
    }

    #[test]
    fn basin_id_usable_as_hashset_key() {
        let mut basins = HashSet::new();
        basins.insert(BasinId::new("a"));
        basins.insert(BasinId::new("b"));
        basins.insert(BasinId::new("a"));
        assert_eq!(basins.len(), 2);
        assert!(basins.contains(&BasinId::new("a")));
    }

    #[test]
    fn grid_label_usable_as_hashmap_key() {
        let mut grids: HashMap<GridLabel, usize> = HashMap::new();
        grids.insert(GridLabel::new("era5"), 1);
        grids.insert(GridLabel::new("chirps"), 2);
        assert_eq!(grids.get(&GridLabel::new("era5")), Some(&1));
        assert_eq!(grids.get(&GridLabel::new("chirps")), Some(&2));
    }
}
