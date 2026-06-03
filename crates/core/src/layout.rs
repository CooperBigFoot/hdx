//! The basin-first hive walk into a typed [`LayoutModel`] (spec §4, architecture §1).
//!
//! HDX is the **hive-partition contract generalized by data shape**: the directory
//! structure *is* the contract (spec §4). This module turns a dataset directory
//! into a typed in-memory [`LayoutModel`] by reading **structure only** — it opens
//! no parquet, no Zarr, no COG and reads no bytes inside any artifact. It records:
//!
//! - the two **root rollups** ([`scalar_static.parquet`](RootRollupKind::ScalarStatic)
//!   and [`outlines.geoparquet`](RootRollupKind::Outlines)): present-or-absent plus
//!   the resolved path each *would* occupy (spec §4 — the dataset-level rollups).
//! - the enumerated `basin=<id>` directories, with the **folder id** parsed out of
//!   each directory name (spec §3 — the `basin=<id>` folder gives *locality*, while
//!   the in-file `basin_id` is *authority*; this walk reads only the folder id).
//! - for each basin, its artifact paths: `scalar_dynamic.parquet` (present/absent +
//!   path) and the `gridded_static/` / `gridded_dynamic/` **subtree** presence +
//!   paths, recorded for the gridded readers — this walk does **not** descend into them.
//!
//! ## Records facts, enforces nothing
//!
//! The walk is the **discovery** spine the scalar reader and the discovery assembler
//! hang facts on. It **enforces no spec §14 check**: a missing root rollup is
//! recorded as [`absent`](RootRollup::is_present) and the walk still succeeds; the
//! gridded-subtree present-vs-absent distinction is recorded (spec §14 L2), never
//! decided here. The only error the walk raises is the structural
//! [`CoreError::LayoutWalk`] when the dataset path itself is not a readable directory.
//!
//! ## Hidden / OS-cruft entries are ignored
//!
//! [`is_ignored_entry`] filters every directory entry: any name beginning with `.`
//! (dotfiles and dot-dirs: `.DS_Store`, `.gitkeep`, `.git`, `.ipynb_checkpoints`, …)
//! is skipped, so working-tree cruft is **never** enumerated as an HDX path or a
//! basin dir. Such entries are not HDX paths, so they must not be counted as stray
//! by the later L3 stray-file check (spec §14 L3 — "no stray/ragged files").
//!
//! ## Glossary
//!
//! | Term | Meaning |
//! |---|---|
//! | root rollup | a dataset-level artifact at the dataset root (`scalar_static.parquet`, `outlines.geoparquet`) — spec §4 |
//! | basin dir | a `basin=<id>` partition directory holding one basin's per-basin data — spec §4 |
//! | folder id | the `<id>` parsed from a `basin=<id>` directory name (locality, not authority) — spec §3 |
//! | gridded subtree | the `gridded_static/` / `gridded_dynamic/` directory under a basin (paths recorded, parsed by the gridded readers) — spec §4 |

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use tracing::{debug, info, instrument, warn};

use crate::error::CoreError;
use crate::newtypes::BasinId;

/// The file name of the static-scalar root rollup (spec §4).
const SCALAR_STATIC_FILE: &str = "scalar_static.parquet";
/// The file name of the outlines root rollup (spec §4).
const OUTLINES_FILE: &str = "outlines.geoparquet";
/// The per-basin dynamic-scalar artifact file name (spec §4).
const SCALAR_DYNAMIC_FILE: &str = "scalar_dynamic.parquet";
/// The per-basin static-gridded subtree directory name (spec §4).
const GRIDDED_STATIC_DIR: &str = "gridded_static";
/// The per-basin dynamic-gridded subtree directory name (spec §4).
const GRIDDED_DYNAMIC_DIR: &str = "gridded_dynamic";
/// The prefix of a basin partition directory name: `basin=<id>` (spec §3/§4).
const BASIN_DIR_PREFIX: &str = "basin=";

/// Which dataset-level root rollup a [`RootRollup`] describes (spec §4).
///
/// An enum, never a `bool`, so the two rollups are self-documenting at every call
/// site (architecture §3.3 — enums over booleans for domain states).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootRollupKind {
    /// `scalar_static.parquet` — the dataset-level static-scalar rollup table.
    ScalarStatic,
    /// `outlines.geoparquet` — the dataset-level basin-outlines table.
    Outlines,
}

impl RootRollupKind {
    /// Returns the on-disk file name this rollup occupies at the dataset root.
    pub fn file_name(&self) -> &'static str {
        match self {
            RootRollupKind::ScalarStatic => SCALAR_STATIC_FILE,
            RootRollupKind::Outlines => OUTLINES_FILE,
        }
    }
}

/// A single root-rollup **presence fact** (spec §4/§14 L1): which rollup, the path
/// it occupies (or would occupy), and whether it is present.
///
/// This records a *fact*, never a verdict: an absent rollup is reported as
/// `present == false` and the walk still succeeds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootRollup {
    kind: RootRollupKind,
    path: PathBuf,
    present: bool,
}

impl RootRollup {
    /// Returns which rollup this is.
    pub fn kind(&self) -> RootRollupKind {
        self.kind
    }

    /// Borrows the path the rollup occupies (or *would* occupy, if absent).
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns `true` iff the rollup file is present at the dataset root.
    pub fn is_present(&self) -> bool {
        self.present
    }
}

/// An optional artifact path: the path it would occupy, plus whether it exists.
///
/// Used for a basin's `scalar_dynamic.parquet` and its `gridded_static/` /
/// `gridded_dynamic/` subtrees. Records the path **unconditionally** (so the gridded
/// readers can find it) and the presence as a fact; it decides nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactPath {
    path: PathBuf,
    present: bool,
}

impl ArtifactPath {
    /// Borrows the path this artifact occupies (or *would* occupy, if absent).
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns `true` iff the artifact (file or subtree directory) exists.
    pub fn is_present(&self) -> bool {
        self.present
    }
}

/// One discovered `basin=<id>` directory and its per-basin artifact paths (spec §4).
///
/// Carries the **folder id** ([`BasinId`]) parsed from the directory name (spec §3
/// — locality, not the authoritative in-file id), the directory path, and the
/// per-basin artifact facts: the `scalar_dynamic.parquet` path (present/absent) and
/// the two gridded subtree paths (present/absent), recorded for the gridded readers.
///
/// It records facts; it does **not** decide L2 (spec §14 L2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BasinDir {
    folder_id: BasinId,
    path: PathBuf,
    scalar_dynamic: ArtifactPath,
    gridded_static: ArtifactPath,
    gridded_dynamic: ArtifactPath,
}

impl BasinDir {
    /// Borrows the folder id parsed from the `basin=<id>` directory name (spec §3).
    ///
    /// This is the **locality** id; the authoritative in-file `basin_id` column is
    /// read by the scalar reader and paired with this for the I2 cross-check (spec §14).
    pub fn folder_id(&self) -> &BasinId {
        &self.folder_id
    }

    /// Borrows the basin directory's path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Borrows the `scalar_dynamic.parquet` artifact fact (path + presence).
    pub fn scalar_dynamic(&self) -> &ArtifactPath {
        &self.scalar_dynamic
    }

    /// Borrows the `gridded_static/` subtree fact (path + presence).
    pub fn gridded_static(&self) -> &ArtifactPath {
        &self.gridded_static
    }

    /// Borrows the `gridded_dynamic/` subtree fact (path + presence).
    pub fn gridded_dynamic(&self) -> &ArtifactPath {
        &self.gridded_dynamic
    }
}

/// The typed in-memory model of one dataset's on-disk layout (spec §4).
///
/// Produced by [`walk_layout`]; consumed by the scalar reader and the discovery
/// assembler. It holds the two root-rollup presence facts and the enumerated basins.
/// It is **inert/agnostic** (spec §1): every field is a structural path or presence
/// fact — no transform, role, semantic type, or provenance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutModel {
    root: PathBuf,
    scalar_static: RootRollup,
    outlines: RootRollup,
    basins: Vec<BasinDir>,
}

impl LayoutModel {
    /// Borrows the dataset root directory that was walked.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Borrows the `scalar_static.parquet` root-rollup presence fact (spec §4 L1).
    pub fn scalar_static(&self) -> &RootRollup {
        &self.scalar_static
    }

    /// Borrows the `outlines.geoparquet` root-rollup presence fact (spec §4 L1).
    pub fn outlines(&self) -> &RootRollup {
        &self.outlines
    }

    /// Borrows the enumerated `basin=<id>` directories, in stable (sorted) order.
    pub fn basins(&self) -> &[BasinDir] {
        &self.basins
    }
}

/// Returns `true` if a directory entry name is hidden / OS cruft and MUST be
/// skipped by the walk (spec §14 L3).
///
/// Any name beginning with `.` is ignored: dotfiles and dot-directories such as
/// `.DS_Store`, `.gitkeep`, `.git`, `.ipynb_checkpoints`, and editor/OS scratch
/// files. Such entries are never HDX paths, so the walk must not enumerate them as
/// basin dirs or count them as stray files. A genuinely empty name (which the
/// filesystem never yields) is also treated as ignored, defensively.
pub fn is_ignored_entry(name: &str) -> bool {
    name.is_empty() || name.starts_with('.')
}

/// Parses a `basin=<id>` directory name into its folder id, or `None` (spec §3/§4).
///
/// Recognizes exactly the `basin=<id>` pattern: the literal prefix `basin=` followed
/// by a **non-empty** id. The id is wrapped verbatim into a [`BasinId`] — HDX
/// parses none of its contents (it is an opaque producer string, spec §3). Any name
/// that does not match returns `None` so the walk skips it:
///
/// - `basin=01013500` → `Some(BasinId("01013500"))`
/// - `basin=` → `None` (empty id)
/// - `basinx`, `basin`, `scalar_static.parquet`, `.DS_Store` → `None`
pub fn parse_basin_dir_name(name: &str) -> Option<BasinId> {
    let id = name.strip_prefix(BASIN_DIR_PREFIX)?;
    if id.is_empty() {
        None
    } else {
        Some(BasinId::new(id))
    }
}

/// Builds an [`ArtifactPath`] for a child of `dir`, recording its existence as a fact.
///
/// The path is recorded unconditionally (so it can be resolved even when absent);
/// presence is read from the filesystem. No bytes inside the artifact are read.
fn artifact_at(dir: &Path, name: &str) -> ArtifactPath {
    let path = dir.join(name);
    let present = path.exists();
    ArtifactPath { path, present }
}

/// Reads one basin directory into a [`BasinDir`], recording its artifact paths.
///
/// `folder_id` is the already-parsed id; `path` is the basin directory. This reads
/// only the *presence* of `scalar_dynamic.parquet` and the two gridded subtrees — it
/// descends into neither.
fn read_basin_dir(folder_id: BasinId, path: PathBuf) -> BasinDir {
    let scalar_dynamic = artifact_at(&path, SCALAR_DYNAMIC_FILE);
    let gridded_static = artifact_at(&path, GRIDDED_STATIC_DIR);
    let gridded_dynamic = artifact_at(&path, GRIDDED_DYNAMIC_DIR);

    debug!(
        basin = folder_id.as_str(),
        scalar_dynamic = scalar_dynamic.is_present(),
        gridded_static = gridded_static.is_present(),
        gridded_dynamic = gridded_dynamic.is_present(),
        "recorded basin artifact paths"
    );

    BasinDir {
        folder_id,
        path,
        scalar_dynamic,
        gridded_static,
        gridded_dynamic,
    }
}

/// Walks a dataset directory into a typed [`LayoutModel`] (spec §4, architecture §1).
///
/// Reads **structure only** — no parquet/Zarr/COG bytes. It records the two
/// root-rollup presence facts, enumerates the `basin=<id>` directories (parsing each
/// folder id via [`parse_basin_dir_name`]), and for each records its per-basin
/// artifact paths (`scalar_dynamic.parquet` + the two gridded subtrees). Hidden /
/// OS-cruft entries are skipped via [`is_ignored_entry`] so they are never
/// enumerated as basin dirs (spec §14 L3). The returned basins are sorted by folder
/// id for a stable, reproducible model.
///
/// This **records facts, enforces nothing**: an absent root rollup is reported (not
/// raised), and the gridded-subtree present-vs-absent distinction is recorded (spec
/// §14 L2). The only failure is structural — see Errors.
///
/// # Errors
///
/// | Condition | Error |
/// |---|---|
/// | `path` does not exist, is not a directory, or its entries cannot be listed | [`CoreError::LayoutWalk`] (with `path` echoed and `detail` from the IO error) |
#[instrument(fields(path = %path.as_ref().display()))]
pub fn walk_layout(path: impl AsRef<Path>) -> Result<LayoutModel, CoreError> {
    let root = path.as_ref();

    // Reject a non-directory / unreadable path up front, before recording any fact.
    let metadata = fs::metadata(root).map_err(|e| CoreError::LayoutWalk {
        path: root.display().to_string(),
        detail: e.to_string(),
    })?;
    if !metadata.is_dir() {
        return Err(CoreError::LayoutWalk {
            path: root.display().to_string(),
            detail: "path is not a directory".to_string(),
        });
    }

    // Root rollups: presence facts, paths recorded unconditionally (spec §4 L1).
    let scalar_static = RootRollup {
        kind: RootRollupKind::ScalarStatic,
        path: root.join(SCALAR_STATIC_FILE),
        present: root.join(SCALAR_STATIC_FILE).is_file(),
    };
    let outlines = RootRollup {
        kind: RootRollupKind::Outlines,
        path: root.join(OUTLINES_FILE),
        present: root.join(OUTLINES_FILE).is_file(),
    };

    // Enumerate the root directory once, skipping ignored entries, collecting only
    // the `basin=<id>` directories (spec §3/§4).
    let entries = fs::read_dir(root).map_err(|e| CoreError::LayoutWalk {
        path: root.display().to_string(),
        detail: e.to_string(),
    })?;

    let mut basins: Vec<BasinDir> = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| CoreError::LayoutWalk {
            path: root.display().to_string(),
            detail: e.to_string(),
        })?;
        let entry_path = entry.path();

        let name = match entry_path.file_name().and_then(OsStr::to_str) {
            Some(name) => name,
            // A non-UTF-8 entry name is not an HDX path; skip it rather than fail.
            None => {
                warn!("skipping directory entry with a non-UTF-8 name");
                continue;
            }
        };

        if is_ignored_entry(name) {
            debug!(entry = name, "skipping hidden / OS-cruft entry");
            continue;
        }

        let Some(folder_id) = parse_basin_dir_name(name) else {
            // Not a `basin=<id>` dir (e.g. the root rollups themselves) — skip; it
            // is handled above or is irrelevant to the basin enumeration.
            continue;
        };

        // A `basin=<id>` *file* (not a directory) is not a basin dir; skip it.
        if !entry_path.is_dir() {
            debug!(
                entry = name,
                "skipping `basin=` entry that is not a directory"
            );
            continue;
        }

        basins.push(read_basin_dir(folder_id, entry_path));
    }

    // Stable order: sort by folder id so the model is reproducible across walks.
    basins.sort_by(|a, b| a.folder_id().as_str().cmp(b.folder_id().as_str()));

    info!(
        scalar_static = scalar_static.is_present(),
        outlines = outlines.is_present(),
        basins = basins.len(),
        "walked dataset layout"
    );

    Ok(LayoutModel {
        root: root.to_path_buf(),
        scalar_static,
        outlines,
        basins,
    })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use crate::error::CoreError;
    use crate::layout::{RootRollupKind, is_ignored_entry, parse_basin_dir_name, walk_layout};
    use crate::newtypes::BasinId;

    /// Resolves a path under the committed `conformance/` fixture tree.
    ///
    /// `CARGO_MANIFEST_DIR` is `crates/core`; the fixtures live two levels up at the
    /// workspace root, so the walk is exercised against the real conformance trees.
    fn conformance(rel: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../conformance")
            .join(rel)
    }

    #[test]
    fn parse_basin_dir_name_accepts_a_well_formed_id() {
        assert_eq!(
            parse_basin_dir_name("basin=01013500"),
            Some(BasinId::new("01013500"))
        );
        assert_eq!(
            parse_basin_dir_name("basin=0001"),
            Some(BasinId::new("0001"))
        );
    }

    #[test]
    fn parse_basin_dir_name_rejects_non_matching_names() {
        for name in [
            "basinx",
            "basin",
            "basin=",
            "scalar_static.parquet",
            ".DS_Store",
            "",
        ] {
            assert_eq!(
                parse_basin_dir_name(name),
                None,
                "{name:?} must not parse as a basin dir"
            );
        }
    }

    #[test]
    fn is_ignored_entry_skips_dot_and_os_cruft() {
        for name in [
            ".DS_Store",
            ".gitkeep",
            ".git",
            ".ipynb_checkpoints",
            ".hidden",
            "",
        ] {
            assert!(is_ignored_entry(name), "{name:?} must be ignored");
        }
    }

    #[test]
    fn is_ignored_entry_keeps_hdx_names() {
        for name in [
            "basin=0001",
            "scalar_static.parquet",
            "outlines.geoparquet",
            "manifest.json",
            "gridded_static",
        ] {
            assert!(!is_ignored_entry(name), "{name:?} must NOT be ignored");
        }
    }

    #[test]
    fn walks_valid_minimal_with_three_basins_and_both_rollups() {
        let model = walk_layout(conformance("valid/minimal"))
            .expect("the valid fixture must walk without error");

        // Both root rollups are present (spec §4 L1).
        assert!(model.scalar_static().is_present());
        assert_eq!(model.scalar_static().kind(), RootRollupKind::ScalarStatic);
        assert!(model.outlines().is_present());
        assert_eq!(model.outlines().kind(), RootRollupKind::Outlines);

        // Exactly three basins, enumerated in stable order with folder ids parsed.
        let ids: Vec<&str> = model
            .basins()
            .iter()
            .map(|b| b.folder_id().as_str())
            .collect();
        assert_eq!(ids, vec!["0001", "0002", "0003"]);

        // Each basin records its scalar_dynamic + gridded subtree paths.
        for basin in model.basins() {
            assert!(
                basin.scalar_dynamic().is_present(),
                "basin {} must have scalar_dynamic.parquet",
                basin.folder_id().as_str()
            );
            // The gridded subtree paths are recorded (and present in this fixture)
            // for the gridded readers — the walk does not descend into them.
            assert!(basin.gridded_static().is_present());
            assert!(basin.gridded_dynamic().is_present());
            assert!(basin.gridded_static().path().ends_with("gridded_static"));
            assert!(basin.gridded_dynamic().path().ends_with("gridded_dynamic"));
        }
    }

    #[test]
    fn missing_root_rollup_is_recorded_not_raised() {
        let model = walk_layout(conformance("invalid/missing-root-rollup"))
            .expect("the walk records the absent rollup, it does NOT fail (L1 is enforced later)");

        // `outlines.geoparquet` is the absent rollup in this fixture; the fact is
        // recorded as absent, and the walk still succeeds.
        assert!(model.scalar_static().is_present());
        assert!(
            !model.outlines().is_present(),
            "the missing rollup must be recorded as absent"
        );

        // Basins still enumerate despite the missing root rollup.
        let ids: Vec<&str> = model
            .basins()
            .iter()
            .map(|b| b.folder_id().as_str())
            .collect();
        assert_eq!(ids, vec!["0001", "0002", "0003"]);
    }

    #[test]
    fn wrong_format_version_walks_identically_to_valid() {
        // The mutation lives only in manifest.json, which the walk never reads, so
        // the layout model matches the valid tree's shape.
        let model = walk_layout(conformance("invalid/wrong-format-version"))
            .expect("the wrong-format-version tree must walk without error");

        assert!(model.scalar_static().is_present());
        assert!(model.outlines().is_present());
        let ids: Vec<&str> = model
            .basins()
            .iter()
            .map(|b| b.folder_id().as_str())
            .collect();
        assert_eq!(ids, vec!["0001", "0002", "0003"]);
    }

    #[test]
    fn ignores_hidden_and_os_cruft_at_root_and_in_basin() {
        // Synthesize a tiny tree in a temp dir, seeded with cruft at the root and
        // inside a basin dir, and assert the walk enumerates only HDX paths.
        let base = std::env::temp_dir().join(format!(
            "hdx-layout-cruft-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        fs::create_dir_all(&base).expect("temp dataset dir must be creatable");

        // Root: the two rollups, a real basin, plus cruft (dotfiles + .git dir).
        fs::write(base.join("scalar_static.parquet"), b"x").expect("write rollup");
        fs::write(base.join("outlines.geoparquet"), b"x").expect("write rollup");
        fs::write(base.join(".DS_Store"), b"x").expect("write cruft");
        fs::write(base.join(".gitkeep"), b"x").expect("write cruft");
        fs::write(base.join(".strayfile"), b"x").expect("write cruft");
        fs::create_dir_all(base.join(".git")).expect("create cruft dir");
        // A dot-prefixed `basin=` name is still cruft and must be skipped.
        fs::create_dir_all(base.join(".basin=9999")).expect("create dot-basin cruft");

        let basin = base.join("basin=0001");
        fs::create_dir_all(&basin).expect("create basin dir");
        fs::write(basin.join("scalar_dynamic.parquet"), b"x").expect("write scalar");
        fs::create_dir_all(basin.join("gridded_static")).expect("create gridded_static");
        fs::create_dir_all(basin.join("gridded_dynamic")).expect("create gridded_dynamic");
        // Cruft inside the basin dir — must not affect enumeration.
        fs::write(basin.join(".DS_Store"), b"x").expect("write basin cruft");
        fs::write(basin.join(".gitkeep"), b"x").expect("write basin cruft");

        let model = walk_layout(&base).expect("the seeded tree must walk");

        // Exactly one basin: the dot-prefixed `.basin=9999` cruft is NOT enumerated.
        let ids: Vec<&str> = model
            .basins()
            .iter()
            .map(|b| b.folder_id().as_str())
            .collect();
        assert_eq!(ids, vec!["0001"], "only the real basin is enumerated");

        assert!(model.scalar_static().is_present());
        assert!(model.outlines().is_present());
        let only = &model.basins()[0];
        assert!(only.scalar_dynamic().is_present());
        assert!(only.gridded_static().is_present());
        assert!(only.gridded_dynamic().is_present());

        fs::remove_dir_all(&base).expect("temp dir cleanup");
    }

    #[test]
    fn walk_on_nonexistent_path_returns_layout_walk() {
        match walk_layout("/no/such/hdx/dataset/path/at/all") {
            Err(CoreError::LayoutWalk { path, detail }) => {
                assert!(path.contains("/no/such/hdx/dataset"));
                assert!(!detail.is_empty(), "the IO detail must be populated");
            }
            other => panic!("expected CoreError::LayoutWalk, got {other:?}"),
        }
    }

    #[test]
    fn walk_on_a_file_returns_layout_walk() {
        // A file (not a directory) is a structural failure of the walk.
        let file =
            std::env::temp_dir().join(format!("hdx-layout-notadir-{}.tmp", std::process::id()));
        fs::write(&file, b"not a directory").expect("write temp file");

        let result = walk_layout(&file);
        fs::remove_file(&file).ok();

        match result {
            Err(CoreError::LayoutWalk { path, detail }) => {
                assert!(path.contains("hdx-layout-notadir"));
                assert!(!detail.is_empty());
            }
            other => panic!("expected CoreError::LayoutWalk for a file, got {other:?}"),
        }
    }
}
