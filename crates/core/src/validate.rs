//! The `validate` verb — the §14 `MUST` checklist over the discovery layer (spec §10).
//!
//! `validate` is HDX's conformance verb: it runs the §14 `MUST` checklist over the
//! shared discovery model (the same model `describe` reports) and emits a
//! [`ValidationReport`] of per-check outcomes plus an overall `conformant: bool`. It
//! **fails closed** — any `MUST` that *ran* and *failed* makes the dataset
//! non-conformant — while reporting honestly **which checks ran vs were skipped** (the
//! spec §14 enforcement-depth note).
//!
//! ## Report vs error — the load-bearing split (spec §0/§10/§14)
//!
//! A **violated `MUST` that ran** is a recorded [`CheckOutcome`] with
//! [`CheckResult::Fail`] ⇒ `conformant: false` — *never* a returned `Err`. A
//! [`ValidateError`] is reserved for **structural / entry** failures: an unreadable
//! `manifest.json`, the §0 hard version cut, an undecodable present artifact. This split
//! mirrors `describe` and lets the CLI (MS7) map a `ValidateError` to a distinct exit
//! code from a `conformant: false` verdict.
//!
//! ## The §0 entry gate runs FIRST (spec §0 entry discipline)
//!
//! [`validate`] mirrors `describe`'s proven, statically-guaranteed stage order so the §0
//! hard cut precedes any other file read:
//!
//! 1. read `<path>/manifest.json` (a filesystem failure → [`ValidateError::ManifestUnreadable`]);
//! 2. [`Manifest::from_json`] — whose **first** act is the §0/§14 M2 `format_version`
//!    hard cut; an unknown version (or any malformed manifest — M3/M4) returns
//!    **before [`discover`] is ever called** as [`ValidateError::Manifest`];
//! 3. [`discover`] (MS3+MS4) — only now is any other file touched (a structural failure
//!    → [`ValidateError::Discovery`]);
//! 4. build the [`ValidationReport`] by running the §14 rules over the assembled model.
//!
//! ## Scope of this step (MS6-S1)
//!
//! This step freezes the report wire shape + the per-check rule-function surface, and
//! implements every check whose rule is a **pure function over the already-typed
//! discovery model and is falsifiable in-memory without differently-shaped on-disk
//! bytes** (the MED-2 fold): **H1, H2, I3, T1, G1**, plus the entry-gate **M1, M2, M3,
//! M4** (folded into [`Manifest::from_json`]). The cross-file checks (M5, M6, L1–L3, I1,
//! I2, T2, G2, G3, Geo1) are listed in the report as `skipped("not yet wired")` so the
//! report shape already enumerates all **20** §14 ids; MS6-S2 flips those skips to real
//! outcomes. Each S1 check records its **R3 depth class** (every S1 check is
//! [`DepthClass::MetadataDeep`]) and ships a mandatory in-memory negative unit test.
//!
//! ## Glossary
//!
//! | Term | Meaning |
//! |---|---|
//! | [`CheckId`] | one of the 20 §14 `MUST` ids, as a closed enum (never a string) |
//! | [`CheckStatus`] | whether a check `Ran` or was `Skipped` (an enum, never a bool) |
//! | [`CheckResult`] | the verdict of a check that ran: `Pass` or `Fail` (enum, never a bool) |
//! | [`DepthClass`] | the R3 enforcement-depth class: `MetadataDeep` vs `ByteDeep` (arch §7 R3) |
//! | [`CheckOutcome`] | one check's recorded result: id + ran/skip + pass/fail + depth + opaque detail |
//! | [`ValidationReport`] | the full report: the per-check outcomes + `conformant` ("no ran-fail") |
//! | ran / skipped (§14 note) | the report MUST clearly state which checks ran; a skip is honest, with a reason |

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use tracing::{debug, info, instrument};

use crate::discovery::BasinScalar;
use crate::error::ValidateError;
use crate::field::Field;
use crate::gridded_discovery::{Discovery, discover};
use crate::manifest::Manifest;
use crate::newtypes::{BasinId, GridLabel};
use crate::scalar_reader::TimeColumn;

/// A single §14 `MUST` check id — the closed set of the 20 conformance checks (spec §14).
///
/// Ids are an enum (never strings) so a typo cannot mint a non-existent check and the
/// report's id space is exhaustive. [`as_str`](CheckId::as_str) yields the stable spec id
/// (`"M1"`…`"Geo1"`) for the wire shape (MS6-S3 serializes it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckId {
    /// `manifest.json` exists, is valid JSON, `format_version` read first (§14 M1).
    M1,
    /// `format_version == "0.1"`; any other value rejected outright (§14 M2 hard cut).
    M2,
    /// Exactly the six floor fields are present; no derivable fields (§14 M3).
    M3,
    /// `created_at` is RFC 3339; `crs`, `cadence` are non-empty strings (§14 M4).
    M4,
    /// `crs` matches the CRS carried in every georeferenced file (§14 M5).
    M5,
    /// `cadence` is consistent with the realized `time` axes (§14 M6).
    M6,
    /// `scalar_static.parquet` and `outlines.geoparquet` exist at the root (§14 L1).
    L1,
    /// Every basin dir matches `basin=<id>` and carries its required artifacts (§14 L2).
    L2,
    /// No stray/ragged files; absence of a field is NaN, never a missing file (§14 L3).
    L3,
    /// `basin_id` is a real in-file column in every required artifact (§14 I1).
    I1,
    /// In-file `basin_id` agrees with the `basin=<id>` folder (§14 I2).
    I2,
    /// `basin_id` is unique within the dataset (§14 I3).
    I3,
    /// Every basin has the identical field schema (§14 H1).
    H1,
    /// The grid-label set is identical across basins (§14 H2).
    H2,
    /// The scalar `time` column is named `time`, a timestamp, non-null, sorted (§14 T1).
    T1,
    /// Within each basin, scalar and gridded dynamic artifacts share the axis (§14 T2).
    T2,
    /// One artifact = one grid; fields self-name; no positional channel axis (§14 G1).
    G1,
    /// A shared grid label across subtrees implies cell-for-cell alignment (§14 G2).
    G2,
    /// Zarr is CF-georeferenced; COG carries standard georef tags (§14 G3).
    G3,
    /// `outlines.geoparquet` has rows `(basin_id, delineation, geometry)` (§14 Geo1).
    Geo1,
}

impl CheckId {
    /// Returns the stable spec id string (`"M1"`…`"Geo1"`) for the wire shape.
    pub fn as_str(&self) -> &'static str {
        match self {
            CheckId::M1 => "M1",
            CheckId::M2 => "M2",
            CheckId::M3 => "M3",
            CheckId::M4 => "M4",
            CheckId::M5 => "M5",
            CheckId::M6 => "M6",
            CheckId::L1 => "L1",
            CheckId::L2 => "L2",
            CheckId::L3 => "L3",
            CheckId::I1 => "I1",
            CheckId::I2 => "I2",
            CheckId::I3 => "I3",
            CheckId::H1 => "H1",
            CheckId::H2 => "H2",
            CheckId::T1 => "T1",
            CheckId::T2 => "T2",
            CheckId::G1 => "G1",
            CheckId::G2 => "G2",
            CheckId::G3 => "G3",
            CheckId::Geo1 => "Geo1",
        }
    }
}

/// The full ordered list of the 20 §14 check ids, in spec order.
///
/// Used by the verb to enumerate every id in the report (the S1-owned ones with a real
/// outcome, the cross-file ones with a `skipped` placeholder) so the report shape lists
/// every §14 id from S1 onward.
const ALL_CHECK_IDS: [CheckId; 20] = [
    CheckId::M1,
    CheckId::M2,
    CheckId::M3,
    CheckId::M4,
    CheckId::M5,
    CheckId::M6,
    CheckId::L1,
    CheckId::L2,
    CheckId::L3,
    CheckId::I1,
    CheckId::I2,
    CheckId::I3,
    CheckId::H1,
    CheckId::H2,
    CheckId::T1,
    CheckId::T2,
    CheckId::G1,
    CheckId::G2,
    CheckId::G3,
    CheckId::Geo1,
];

/// Whether a check `Ran` or was `Skipped` (an enum, never a bool — architecture §3.3).
///
/// The spec §14 enforcement-depth note requires the validator to clearly report which
/// checks ran; a `Skipped` check is an honest deferral, always paired with a reason in
/// its [`CheckOutcome`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckStatus {
    /// The check executed and produced a [`CheckResult`].
    Ran,
    /// The check did not execute (an honest R3 deferral, with a recorded reason).
    Skipped,
}

/// The verdict of a check that ran: `Pass` or `Fail` (an enum, never a bool).
///
/// Only a [`CheckStatus::Ran`] check carries a `CheckResult`; a `Fail` of any `MUST`
/// that ran makes the dataset non-conformant (fail-closed, spec §14).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckResult {
    /// The `MUST` held.
    Pass,
    /// The `MUST` was violated — the dataset is non-conformant (fail-closed).
    Fail,
}

/// The R3 enforcement-depth class of a check (architecture §7 R3).
///
/// Records how deep into the bytes a check reached. Every MS6-S1 check is
/// [`DepthClass::MetadataDeep`] — it runs over the discovery layer + the 1-D index reads
/// MS3/MS4 already perform, never a gridded chunk or pixel raster (LOW-3). The
/// [`DepthClass::ByteDeep`] class is reserved for the genuinely byte-level legs (e.g. the
/// MS6-S2 per-basin axis-regularity leg) that v0.1 honestly skips.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DepthClass {
    /// The check was decided from metadata + 1-D index reads only (no chunk/pixel read).
    MetadataDeep,
    /// The check would require a byte-level (chunk/pixel/full-axis) read to decide.
    ByteDeep,
}

impl DepthClass {
    /// Returns the stable wire string (`"metadata_deep"` / `"byte_deep"`) for the report.
    pub fn as_str(&self) -> &'static str {
        match self {
            DepthClass::MetadataDeep => "metadata_deep",
            DepthClass::ByteDeep => "byte_deep",
        }
    }
}

/// One §14 check's recorded result (spec §14).
///
/// Carries the check `id`, whether it `Ran` or was `Skipped`, the `Pass`/`Fail`
/// [`result`](CheckOutcome::result) (`Some` iff it ran), its R3 [`depth`](CheckOutcome::depth)
/// class, and an opaque `detail` string (a fail reason or a skip reason). Fields are
/// private; build outcomes through the three constructors
/// ([`ran_pass`](CheckOutcome::ran_pass), [`ran_fail`](CheckOutcome::ran_fail),
/// [`skipped`](CheckOutcome::skipped)) so a `Skipped` check can never carry a result and a
/// `Ran` check always does.
///
/// It is **inert/agnostic** (spec §1): it carries only the structural facts of the check
/// (id / ran-skip / pass-fail / depth / opaque detail) — no derived domain field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckOutcome {
    id: CheckId,
    status: CheckStatus,
    result: Option<CheckResult>,
    depth: DepthClass,
    detail: Option<String>,
}

impl CheckOutcome {
    /// Builds a `ran:pass` outcome (the `MUST` held) at the given depth class.
    pub fn ran_pass(id: CheckId, depth: DepthClass) -> Self {
        Self {
            id,
            status: CheckStatus::Ran,
            result: Some(CheckResult::Pass),
            depth,
            detail: None,
        }
    }

    /// Builds a `ran:fail` outcome (the `MUST` was violated) with an opaque detail.
    pub fn ran_fail(id: CheckId, depth: DepthClass, detail: impl Into<String>) -> Self {
        Self {
            id,
            status: CheckStatus::Ran,
            result: Some(CheckResult::Fail),
            depth,
            detail: Some(detail.into()),
        }
    }

    /// Builds a `skipped` outcome (an honest R3 deferral) with a recorded reason.
    pub fn skipped(id: CheckId, depth: DepthClass, reason: impl Into<String>) -> Self {
        Self {
            id,
            status: CheckStatus::Skipped,
            result: None,
            depth,
            detail: Some(reason.into()),
        }
    }

    /// Returns the check id.
    pub fn id(&self) -> CheckId {
        self.id
    }

    /// Returns whether the check ran or was skipped.
    pub fn status(&self) -> CheckStatus {
        self.status
    }

    /// Returns the pass/fail verdict, or `None` for a skipped check.
    pub fn result(&self) -> Option<CheckResult> {
        self.result
    }

    /// Returns the R3 depth class.
    pub fn depth(&self) -> DepthClass {
        self.depth
    }

    /// Borrows the opaque detail / reason string, if any.
    pub fn detail(&self) -> Option<&str> {
        self.detail.as_deref()
    }
}

/// The full validation report: the per-check outcomes + the overall verdict (spec §14).
///
/// `conformant` is computed **by construction** as "**no check that `Ran` has
/// `result == Fail`**" — a `Skipped` check never flips it (fail-closed applies only to a
/// violated `MUST` that ran, spec §14). Fields are private; build a report with
/// [`ValidationReport::from_outcomes`] (which recomputes `conformant`) and read it via
/// [`checks`](ValidationReport::checks) / [`conformant`](ValidationReport::conformant) /
/// [`find`](ValidationReport::find).
///
/// It is **inert/agnostic** (spec §1): it carries only the check outcomes and the boolean
/// verdict — no derived domain field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationReport {
    checks: Vec<CheckOutcome>,
    conformant: bool,
}

impl ValidationReport {
    /// Builds a report from the per-check outcomes, computing `conformant` as
    /// "no check that ran failed".
    ///
    /// This is the single place `conformant` is derived, so it can never disagree with
    /// the recorded outcomes. A [`CheckStatus::Skipped`] check is ignored by the verdict
    /// (it has no `result`); only a [`CheckStatus::Ran`] check with
    /// [`CheckResult::Fail`] makes the report non-conformant.
    pub fn from_outcomes(checks: Vec<CheckOutcome>) -> Self {
        let conformant = !checks.iter().any(|c| c.result() == Some(CheckResult::Fail));
        Self { checks, conformant }
    }

    /// Borrows the per-check outcomes, in spec order.
    pub fn checks(&self) -> &[CheckOutcome] {
        &self.checks
    }

    /// Returns the overall verdict: `true` iff no check that ran failed (spec §14).
    pub fn conformant(&self) -> bool {
        self.conformant
    }

    /// Returns the outcome for a given check id, if present.
    pub fn find(&self, id: CheckId) -> Option<&CheckOutcome> {
        self.checks.iter().find(|c| c.id() == id)
    }
}

// --- The pure in-memory rule functions (MS6-S1) --------------------------------------
//
// Each rule is a pure function over borrowed pieces of the already-typed discovery model
// and returns a single `CheckOutcome`. Each is falsifiable in-memory without
// differently-shaped on-disk bytes (the MED-2 fold), and each is `MetadataDeep` (no
// chunk/pixel read). M3/M4 are *not* here — they are folded into the entry gate via
// `Manifest::from_json` (a parsed manifest ⇒ M3/M4 pass; a parse error ⇒ a typed `Err`),
// and exercised in-memory by calling `from_json` on hand-built JSON strings.

/// Checks H1 — every basin has the identical field schema (spec §5/§14 H1).
///
/// The field schema of a basin is the **ordered** list of its [`Field`]s compared by
/// `(name, quadrant, dtype, grid_label)` — a divergent basin (a renamed field, a dtype
/// mismatch, a missing/extra field, a different quadrant or grid label) ⇒ `ran:fail`
/// (a `RaggedSchema`-style detail naming the first divergent basin). `units` is **not**
/// part of the homogeneity key (it is an opaque annotation, spec §2). R3: `MetadataDeep`.
///
/// Input: one entry per basin, pairing the basin id with its field list (borrowed from
/// the discovery model so the rule stays pure and in-memory-falsifiable).
#[instrument(skip(fields_by_basin))]
pub fn check_h1(fields_by_basin: &[(&BasinId, Vec<&Field>)]) -> CheckOutcome {
    // The signature of a basin's schema: the ordered field identity tuples.
    fn schema_key(
        fields: &[&Field],
    ) -> Vec<(
        String,
        crate::field::Quadrant,
        crate::field::Dtype,
        Option<String>,
    )> {
        fields
            .iter()
            .map(|f| {
                (
                    f.name().as_str().to_string(),
                    f.quadrant(),
                    f.dtype(),
                    f.grid_label().map(|l| l.as_str().to_string()),
                )
            })
            .collect()
    }

    let mut iter = fields_by_basin.iter();
    let Some((first_id, first_fields)) = iter.next() else {
        // No basins ⇒ vacuously homogeneous.
        return CheckOutcome::ran_pass(CheckId::H1, DepthClass::MetadataDeep);
    };
    let reference = schema_key(first_fields);

    for (basin_id, fields) in iter {
        if schema_key(fields) != reference {
            debug!(
                reference_basin = first_id.as_str(),
                divergent_basin = basin_id.as_str(),
                "H1: basin field schema diverges from the dataset schema"
            );
            return CheckOutcome::ran_fail(
                CheckId::H1,
                DepthClass::MetadataDeep,
                format!(
                    "ragged schema: basin {:?} does not share the schema of basin {:?}",
                    basin_id.as_str(),
                    first_id.as_str()
                ),
            );
        }
    }
    CheckOutcome::ran_pass(CheckId::H1, DepthClass::MetadataDeep)
}

/// Checks H2 — the grid-label set is identical across basins (spec §8/§14 H2).
///
/// Each basin contributes a **set** of observed grid labels (static ⊕ dynamic, order- and
/// duplicate-insensitive); a basin whose set differs from the reference ⇒ `ran:fail`
/// (a `GridLabelMismatchAcrossBasins`-style detail). R3: `MetadataDeep`.
///
/// Input: one entry per basin, pairing the basin id with its observed grid labels.
#[instrument(skip(labels_by_basin))]
pub fn check_h2(labels_by_basin: &[(&BasinId, Vec<&GridLabel>)]) -> CheckOutcome {
    fn label_set(labels: &[&GridLabel]) -> BTreeSet<String> {
        labels.iter().map(|l| l.as_str().to_string()).collect()
    }

    let mut iter = labels_by_basin.iter();
    let Some((first_id, first_labels)) = iter.next() else {
        return CheckOutcome::ran_pass(CheckId::H2, DepthClass::MetadataDeep);
    };
    let reference = label_set(first_labels);

    for (basin_id, labels) in iter {
        if label_set(labels) != reference {
            debug!(
                reference_basin = first_id.as_str(),
                divergent_basin = basin_id.as_str(),
                "H2: grid-label set diverges across basins"
            );
            return CheckOutcome::ran_fail(
                CheckId::H2,
                DepthClass::MetadataDeep,
                format!(
                    "grid-label set of basin {:?} differs from basin {:?}",
                    basin_id.as_str(),
                    first_id.as_str()
                ),
            );
        }
    }
    CheckOutcome::ran_pass(CheckId::H2, DepthClass::MetadataDeep)
}

/// Checks I3 — `basin_id` is unique within the dataset (spec §3/§14 I3).
///
/// The in-file `basin_id` values must be all-distinct across the dataset; a duplicate ⇒
/// `ran:fail` (naming the first repeated id). R3: `MetadataDeep`.
///
/// Input: the in-file `basin_id` values (one per basin that surfaced one).
#[instrument(skip(in_file_ids))]
pub fn check_i3(in_file_ids: &[&BasinId]) -> CheckOutcome {
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for id in in_file_ids {
        if !seen.insert(id.as_str()) {
            debug!(duplicate = id.as_str(), "I3: duplicate basin_id");
            return CheckOutcome::ran_fail(
                CheckId::I3,
                DepthClass::MetadataDeep,
                format!("duplicate basin_id {:?} within the dataset", id.as_str()),
            );
        }
    }
    CheckOutcome::ran_pass(CheckId::I3, DepthClass::MetadataDeep)
}

/// Checks T1 — the scalar `time` column is conformant (spec §6.3/§14 T1).
///
/// Every present `time` descriptor must be named `time`, dtype [`Dtype::Timestamp`],
/// **non-nullable**, and **sorted ascending**; any violated leg ⇒ `ran:fail` (a detail
/// naming the failing leg — a `NonMonotonicTime`-style detail for the sort leg). A basin
/// with **no** `time` descriptor (an absent `scalar_dynamic.parquet`) is a §6.1 gap, not a
/// T1 failure, so it is skipped here. R3: `MetadataDeep`.
///
/// [`Dtype::Timestamp`]: crate::field::Dtype::Timestamp
///
/// Input: one entry per basin, pairing the basin id with its `Option<&TimeColumn>`.
#[instrument(skip(per_basin_time))]
pub fn check_t1(per_basin_time: &[(&BasinId, Option<&TimeColumn>)]) -> CheckOutcome {
    use crate::field::Dtype;

    for (basin_id, time) in per_basin_time {
        let Some(time) = time else {
            // A basin with no scalar_dynamic time descriptor is a §6.1 gap, not a T1 fail.
            continue;
        };
        if time.name() != "time" {
            return CheckOutcome::ran_fail(
                CheckId::T1,
                DepthClass::MetadataDeep,
                format!(
                    "basin {:?}: time column is named {:?}, expected \"time\"",
                    basin_id.as_str(),
                    time.name()
                ),
            );
        }
        if time.dtype() != Dtype::Timestamp {
            return CheckOutcome::ran_fail(
                CheckId::T1,
                DepthClass::MetadataDeep,
                format!(
                    "basin {:?}: time column dtype is {:?}, expected timestamp",
                    basin_id.as_str(),
                    time.dtype().as_str()
                ),
            );
        }
        if time.is_nullable() {
            return CheckOutcome::ran_fail(
                CheckId::T1,
                DepthClass::MetadataDeep,
                format!(
                    "basin {:?}: time column is nullable, expected non-nullable",
                    basin_id.as_str()
                ),
            );
        }
        if !time.is_sorted_ascending() {
            return CheckOutcome::ran_fail(
                CheckId::T1,
                DepthClass::MetadataDeep,
                format!("non-monotonic time axis in basin {:?}", basin_id.as_str()),
            );
        }
    }
    CheckOutcome::ran_pass(CheckId::T1, DepthClass::MetadataDeep)
}

/// Checks G1 — every gridded field self-names; no positional channel axis (spec §8/§14 G1).
///
/// G1 verifies the catalog is built so that **one artifact = one grid** and every gridded
/// field carries its own grid label (the CF variable / COG band description *is* the field
/// name). [`Field::new`] already makes a label-less gridded field unrepresentable, so the
/// **in-memory-falsifiable form** of G1 (the MED-2 fold) is: feed the rule a field list and
/// confirm it `ran:pass` **iff every gridded field self-names** (carries `Some(GridLabel)`);
/// a gridded field whose label is absent ⇒ `ran:fail`. Because the invariant holds by
/// construction, a conformant catalog always passes — but the rule is the explicit check
/// that no gridded field was admitted without its label (no positional channel axis).
/// R3: `MetadataDeep`.
///
/// [`Field::new`]: crate::field::Field::new
///
/// Input: the unified field catalog (the gridded entries are the ones tested).
#[instrument(skip(fields))]
pub fn check_g1(fields: &[&Field]) -> CheckOutcome {
    use crate::field::Shape;

    for field in fields {
        if field.quadrant().shape() == Shape::Gridded && field.grid_label().is_none() {
            debug!(
                field = field.name().as_str(),
                "G1: gridded field does not self-name (missing grid label)"
            );
            return CheckOutcome::ran_fail(
                CheckId::G1,
                DepthClass::MetadataDeep,
                format!(
                    "gridded field {:?} does not self-name (no grid label)",
                    field.name().as_str()
                ),
            );
        }
    }
    CheckOutcome::ran_pass(CheckId::G1, DepthClass::MetadataDeep)
}

// --- The verb -----------------------------------------------------------------------

/// Validates a dataset against the §14 `MUST` checklist, returning a [`ValidationReport`]
/// (spec §10/§14).
///
/// Runs the §0 entry gate first (read `manifest.json` → [`Manifest::from_json`] hard cut →
/// [`discover`]), then runs the §14 rules over the assembled discovery model. As of
/// MS6-S1 the report carries real outcomes for the in-memory-falsifiable checks
/// (M1–M4 via the entry gate; H1, H2, I3, T1, G1) and `skipped("not yet wired")`
/// placeholders for the cross-file checks (M5, M6, L1–L3, I1, I2, T2, G2, G3, Geo1), so
/// the report already lists all **20** §14 ids; MS6-S2 flips the placeholders to real outcomes.
///
/// `conformant` is "no check that ran failed" (a skip never flips it). A **violated
/// `MUST`** is a recorded fail outcome, *never* a returned `Err`.
///
/// ## Load-bearing order (spec §0 entry discipline)
///
/// The stages run in a strict, statically-guaranteed order — the §0 hard cut and the
/// manifest boundary-parse happen **before any other file is touched**:
///
/// 1. read `<path>/manifest.json` (a filesystem failure → [`ValidateError::ManifestUnreadable`]);
/// 2. [`Manifest::from_json`] — its **first** act is the §0/§14 M2 hard version cut;
///    an unknown version (or any malformed manifest — M3/M4) returns here as
///    [`ValidateError::Manifest`] **before [`discover`] is reached** (the early `?`
///    makes this a static guarantee, mirroring `describe`);
/// 3. [`discover`] (a structural failure → [`ValidateError::Discovery`]);
/// 4. build the report.
///
/// # Errors
///
/// | Condition | Error |
/// |---|---|
/// | `<path>/manifest.json` is absent or unreadable | [`ValidateError::ManifestUnreadable`] |
/// | `format_version` is not `"0.1"` (the §0 hard cut, evaluated **before** discovery) | [`ValidateError::Manifest`] wrapping [`CoreError::UnknownFormatVersion`](crate::error::CoreError::UnknownFormatVersion) |
/// | the manifest is otherwise malformed (extra/missing field, bad timestamp, empty crs/cadence — M3/M4) | [`ValidateError::Manifest`] |
/// | discovery (layout walk or a metadata reader) fails after the manifest is accepted | [`ValidateError::Discovery`] |
///
/// A violated `MUST` is **never** an error — it is a recorded fail [`CheckOutcome`].
#[instrument(fields(path = %path.as_ref().display()))]
pub fn validate(path: impl AsRef<Path>) -> Result<ValidationReport, ValidateError> {
    let path = path.as_ref();

    // Stage 1 — read manifest.json FIRST (spec §0): before any other file is touched.
    let manifest_path = path.join("manifest.json");
    let manifest_json =
        fs::read_to_string(&manifest_path).map_err(|err| ValidateError::ManifestUnreadable {
            path: manifest_path.display().to_string(),
            detail: err.to_string(),
        })?;
    debug!("read manifest.json");

    // Stage 2 — boundary-parse the manifest. Its FIRST act is the §0/§14 M2 hard version
    // cut; M3/M4 are likewise enforced here. The early `?` makes the §0 hard cut precede
    // `discover` by construction (the discovery call below is unreachable until parse OK).
    let _manifest = Manifest::from_json(&manifest_json).map_err(ValidateError::Manifest)?;
    debug!("manifest boundary-parse passed (M1/M2 hard cut + M3/M4 cleared)");

    // Stage 3 — discovery: only now is any other file in the dataset read.
    let discovery = discover(path).map_err(ValidateError::Discovery)?;
    debug!("discovery complete");

    // Stage 4 — build the report by running the §14 rules over the assembled model.
    let report = build_report(&discovery);
    info!(
        conformant = report.conformant(),
        checks = report.checks().len(),
        "validated dataset"
    );
    Ok(report)
}

/// Assembles the report by running every §14 rule over the discovery model (spec §14).
///
/// Lists all **20** §14 ids in spec order: M1–M4 are `ran:pass` (the entry gate already
/// cleared them — a non-conformant manifest would have returned an `Err` before this
/// function ran); H1, H2, I3, T1, G1 run their in-memory rules; the cross-file checks are
/// `skipped("not yet wired")` placeholders (MS6-S2 flips them to real outcomes). Pure: no
/// IO.
fn build_report(discovery: &Discovery) -> ValidationReport {
    // The S1 in-memory checks, computed from the discovery accessors.
    let fields_by_basin = fields_by_basin(discovery);
    let h1 = check_h1(&fields_by_basin);

    let labels_by_basin = labels_by_basin(discovery);
    let h2 = check_h2(&labels_by_basin);

    let in_file_ids = in_file_basin_ids(discovery);
    let i3 = check_i3(&in_file_ids);

    let per_basin_time = per_basin_time(discovery);
    let t1 = check_t1(&per_basin_time);

    let all_fields = discovery.fields();
    let g1 = check_g1(&all_fields);

    let checks: Vec<CheckOutcome> = ALL_CHECK_IDS
        .iter()
        .map(|&id| match id {
            // M1–M4 are folded into the entry gate: reaching this point means the manifest
            // parsed, so each cleared its boundary check (a violation would have returned
            // `Err` before discovery). Entry-gate checks are `MetadataDeep`.
            CheckId::M1 | CheckId::M2 | CheckId::M3 | CheckId::M4 => {
                CheckOutcome::ran_pass(id, DepthClass::MetadataDeep)
            }
            CheckId::H1 => h1.clone(),
            CheckId::H2 => h2.clone(),
            CheckId::I3 => i3.clone(),
            CheckId::T1 => t1.clone(),
            CheckId::G1 => g1.clone(),
            // The cross-file checks land in MS6-S2; until then they are honest skips so
            // the report shape already lists every §14 id.
            CheckId::M5
            | CheckId::M6
            | CheckId::L1
            | CheckId::L2
            | CheckId::L3
            | CheckId::I1
            | CheckId::I2
            | CheckId::T2
            | CheckId::G2
            | CheckId::G3
            | CheckId::Geo1 => CheckOutcome::skipped(id, DepthClass::MetadataDeep, "not yet wired"),
        })
        .collect();

    ValidationReport::from_outcomes(checks)
}

/// Builds the per-basin field lists (scalar ⊕ gridded) the H1 rule consumes.
///
/// Pairs each basin's folder id with its dynamic-scalar fields followed by its gridded
/// fields, in basin order. The dataset-wide static fields (the `scalar_static` rollup and
/// the gridded catalog) are homogeneous by discovery construction; the per-basin slice the
/// model surfaces today is the dynamic-scalar schema, which is what S1's in-memory H1 leg
/// compares. (MS6-S2 may widen the per-basin schema once the full per-basin catalog is
/// surfaced; the rule signature is already general.)
fn fields_by_basin(discovery: &Discovery) -> Vec<(&BasinId, Vec<&Field>)> {
    discovery
        .scalar()
        .per_basin()
        .iter()
        .map(|basin: &BasinScalar| {
            let fields: Vec<&Field> = basin.fields().iter().collect();
            (basin.basin_id_folder(), fields)
        })
        .collect()
}

/// Builds the per-basin observed grid-label lists the H2 rule consumes.
///
/// Pairs each basin's folder id with the union of its static + dynamic observed grid
/// labels, in basin order.
fn labels_by_basin(discovery: &Discovery) -> Vec<(&BasinId, Vec<&GridLabel>)> {
    discovery
        .gridded()
        .per_basin()
        .iter()
        .map(|basin| {
            let mut labels = basin.static_grid_labels();
            labels.extend(basin.dynamic_grid_labels());
            (basin.basin_id_folder(), labels)
        })
        .collect()
}

/// Builds the in-file `basin_id` list the I3 rule consumes.
///
/// One entry per basin that surfaced an in-file `basin_id` value (a basin with no value is
/// an I1 concern handled in MS6-S2, not an I3 duplicate).
fn in_file_basin_ids(discovery: &Discovery) -> Vec<&BasinId> {
    discovery
        .scalar()
        .per_basin()
        .iter()
        .filter_map(BasinScalar::basin_id_in_file)
        .collect()
}

/// Builds the per-basin `time` descriptor list the T1 rule consumes.
fn per_basin_time(discovery: &Discovery) -> Vec<(&BasinId, Option<&TimeColumn>)> {
    discovery
        .scalar()
        .per_basin()
        .iter()
        .map(|basin| (basin.basin_id_folder(), basin.time()))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use crate::error::{CoreError, ValidateError};
    use crate::field::{Dtype, Field, Quadrant, Units};
    use crate::manifest::Manifest;
    use crate::newtypes::{BasinId, GridLabel};
    use crate::scalar_reader::TimeColumn;

    use crate::validate::{
        CheckId, CheckResult, CheckStatus, DepthClass, ValidationReport, check_g1, check_h1,
        check_h2, check_i3, check_t1, validate,
    };

    /// Resolves a path under the committed `conformance/` fixture tree.
    ///
    /// `CARGO_MANIFEST_DIR` is `crates/core`; the fixtures live two levels up.
    fn conformance(rel: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../conformance")
            .join(rel)
    }

    /// Builds a scalar-dynamic field with the given name and dtype (no grid label).
    fn scalar_field(name: &str, dtype: Dtype) -> Field {
        Field::new(
            crate::newtypes::FieldName::new(name),
            Quadrant::ScalarDynamic,
            dtype,
            Units::none(),
            None,
        )
        .expect("scalar field constructs")
    }

    /// Builds a gridded-dynamic field with the given name, dtype, and grid label.
    fn gridded_field(name: &str, dtype: Dtype, label: &str) -> Field {
        Field::new(
            crate::newtypes::FieldName::new(name),
            Quadrant::GriddedDynamic,
            dtype,
            Units::none(),
            Some(GridLabel::new(label)),
        )
        .expect("gridded field constructs")
    }

    // --- Entry gate -------------------------------------------------------------------

    #[test]
    fn entry_gate_hard_cuts_unknown_format_version_before_discovery() {
        // The §0 hard cut runs before discovery (statically guaranteed by the verb's
        // stage order, mirroring `describe`): the wrong-version fixture errors with
        // UnknownFormatVersion, never a discovery error or a `conformant:false` report.
        match validate(conformance("invalid/wrong-format-version")) {
            Err(ValidateError::Manifest(CoreError::UnknownFormatVersion { found })) => {
                assert_eq!(found, "0.2");
            }
            other => panic!("expected Manifest(UnknownFormatVersion), got {other:?}"),
        }
    }

    #[test]
    fn entry_gate_reports_unreadable_manifest_for_missing_manifest_json() {
        let tmp =
            std::env::temp_dir().join(format!("hdx-validate-no-manifest-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).expect("temp dir");
        match validate(&tmp) {
            Err(ValidateError::ManifestUnreadable { path, .. }) => {
                assert!(path.ends_with("manifest.json"));
            }
            other => panic!("expected ManifestUnreadable, got {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }

    // --- M3 / M4 in-memory negatives (folded into the entry gate) ---------------------

    #[test]
    fn m3_in_memory_negative_seven_field_manifest_rejected() {
        // A hand-built 7-field manifest is rejected at the boundary (M3) — no on-disk
        // bytes: the entry gate maps this to ValidateError::Manifest.
        let seven_field = r#"{
            "format_version": "0.1",
            "name": "x",
            "created_at": "2026-06-01T00:00:00Z",
            "producer_version": "p",
            "crs": "EPSG:4326",
            "cadence": "daily",
            "content_hash": "deadbeef"
        }"#;
        match Manifest::from_json(seven_field) {
            Err(CoreError::ExtraManifestField { field }) => assert_eq!(field, "content_hash"),
            other => panic!("expected ExtraManifestField, got {other:?}"),
        }
    }

    #[test]
    fn m4_in_memory_negative_empty_crs_and_bad_created_at_rejected() {
        // Empty crs ⇒ M4 fail at the boundary.
        let empty_crs = r#"{
            "format_version": "0.1",
            "name": "x",
            "created_at": "2026-06-01T00:00:00Z",
            "producer_version": "p",
            "crs": "",
            "cadence": "daily"
        }"#;
        assert!(matches!(
            Manifest::from_json(empty_crs),
            Err(CoreError::EmptyCrs)
        ));

        // Bad created_at ⇒ M4 fail at the boundary.
        let bad_created_at = r#"{
            "format_version": "0.1",
            "name": "x",
            "created_at": "not-a-date",
            "producer_version": "p",
            "crs": "EPSG:4326",
            "cadence": "daily"
        }"#;
        match Manifest::from_json(bad_created_at) {
            Err(CoreError::InvalidTimestamp { value }) => assert_eq!(value, "not-a-date"),
            other => panic!("expected InvalidTimestamp, got {other:?}"),
        }
    }

    // --- H1 in-memory negative --------------------------------------------------------

    #[test]
    fn h1_negative_on_dtype_mismatch_positive_on_match() {
        let basin_a = BasinId::new("0001");
        let basin_b = BasinId::new("0002");

        // Divergent: one basin's streamflow is F64, the other's F32 ⇒ ran:fail.
        let f64_field = scalar_field("streamflow", Dtype::F64);
        let f32_field = scalar_field("streamflow", Dtype::F32);
        let divergent = vec![(&basin_a, vec![&f64_field]), (&basin_b, vec![&f32_field])];
        let outcome = check_h1(&divergent);
        assert_eq!(outcome.status(), CheckStatus::Ran);
        assert_eq!(outcome.result(), Some(CheckResult::Fail));
        assert_eq!(outcome.depth(), DepthClass::MetadataDeep);

        // Matching: both basins share the identical schema ⇒ ran:pass.
        let a_match = scalar_field("streamflow", Dtype::F64);
        let b_match = scalar_field("streamflow", Dtype::F64);
        let matching = vec![(&basin_a, vec![&a_match]), (&basin_b, vec![&b_match])];
        let outcome = check_h1(&matching);
        assert_eq!(outcome.status(), CheckStatus::Ran);
        assert_eq!(outcome.result(), Some(CheckResult::Pass));
    }

    // --- H2 in-memory negative --------------------------------------------------------

    #[test]
    fn h2_negative_on_divergent_label_set_positive_on_identical() {
        let basin_a = BasinId::new("0001");
        let basin_b = BasinId::new("0002");
        let era5 = GridLabel::new("era5");
        let chirps = GridLabel::new("chirps");

        // {era5} vs {era5, chirps} ⇒ ran:fail.
        let divergent = vec![(&basin_a, vec![&era5]), (&basin_b, vec![&era5, &chirps])];
        let outcome = check_h2(&divergent);
        assert_eq!(outcome.result(), Some(CheckResult::Fail));

        // Identical sets ⇒ ran:pass.
        let identical = vec![(&basin_a, vec![&era5]), (&basin_b, vec![&era5])];
        let outcome = check_h2(&identical);
        assert_eq!(outcome.result(), Some(CheckResult::Pass));
    }

    // --- I3 in-memory negative --------------------------------------------------------

    #[test]
    fn i3_negative_on_duplicate_positive_on_distinct() {
        let a1 = BasinId::new("0001");
        let a2 = BasinId::new("0001");
        let b = BasinId::new("0002");

        // Duplicate 0001 ⇒ ran:fail.
        let dup = vec![&a1, &a2, &b];
        let outcome = check_i3(&dup);
        assert_eq!(outcome.result(), Some(CheckResult::Fail));

        // All-distinct ⇒ ran:pass.
        let distinct = vec![&a1, &b];
        let outcome = check_i3(&distinct);
        assert_eq!(outcome.result(), Some(CheckResult::Pass));
    }

    // --- T1 in-memory negative (per leg) ----------------------------------------------

    #[test]
    fn t1_negative_per_leg_positive_on_conformant() {
        let basin = BasinId::new("0001");

        // Conformant ⇒ ran:pass.
        let good = TimeColumn::new_for_test("time", Dtype::Timestamp, false, true);
        let outcome = check_t1(&[(&basin, Some(&good))]);
        assert_eq!(outcome.result(), Some(CheckResult::Pass));

        // Nullable ⇒ ran:fail.
        let nullable = TimeColumn::new_for_test("time", Dtype::Timestamp, true, true);
        assert_eq!(
            check_t1(&[(&basin, Some(&nullable))]).result(),
            Some(CheckResult::Fail)
        );

        // Not sorted ⇒ ran:fail.
        let unsorted = TimeColumn::new_for_test("time", Dtype::Timestamp, false, false);
        assert_eq!(
            check_t1(&[(&basin, Some(&unsorted))]).result(),
            Some(CheckResult::Fail)
        );

        // dtype != Timestamp ⇒ ran:fail.
        let wrong_dtype = TimeColumn::new_for_test("time", Dtype::I64, false, true);
        assert_eq!(
            check_t1(&[(&basin, Some(&wrong_dtype))]).result(),
            Some(CheckResult::Fail)
        );

        // name != "time" ⇒ ran:fail.
        let wrong_name = TimeColumn::new_for_test("ts", Dtype::Timestamp, false, true);
        assert_eq!(
            check_t1(&[(&basin, Some(&wrong_name))]).result(),
            Some(CheckResult::Fail)
        );

        // A basin with no time descriptor (a §6.1 gap) does not fail T1.
        assert_eq!(
            check_t1(&[(&basin, None)]).result(),
            Some(CheckResult::Pass)
        );
    }

    // --- G1 in-memory-falsifiable form (MED-2) ----------------------------------------

    #[test]
    fn g1_passes_only_when_every_gridded_field_self_names() {
        // A mixed list where every gridded field self-names (carries Some(GridLabel)) ⇒
        // ran:pass. `Field::new` makes a label-less gridded field unrepresentable, so the
        // in-memory-falsifiable form is: the rule passes iff every gridded field has a
        // label. We assert the positive form holds for a conformant mixed catalog.
        let scalar = scalar_field("streamflow", Dtype::F64);
        let gridded = gridded_field("era5_precipitation", Dtype::F32, "era5");
        let fields = vec![&scalar, &gridded];
        let outcome = check_g1(&fields);
        assert_eq!(outcome.status(), CheckStatus::Ran);
        assert_eq!(outcome.result(), Some(CheckResult::Pass));
        assert_eq!(outcome.depth(), DepthClass::MetadataDeep);
    }

    // --- Report shape -----------------------------------------------------------------

    #[test]
    fn report_lists_all_nineteen_ids_and_is_conformant_on_valid_fixture() {
        let report = validate(conformance("valid/minimal")).expect("the valid fixture validates");

        // All §14 ids are present, in spec order. The §14 checklist enumerates 20 ids
        // (M1–M6, L1–L3, I1–I3, H1–H2, T1–T2, G1–G3, Geo1); the planning prose's "19" is
        // an off-by-one — the closed `CheckId` enum is the authoritative count.
        let ids: Vec<CheckId> = report.checks().iter().map(|c| c.id()).collect();
        assert_eq!(ids.len(), 20, "the report lists every §14 id");
        let expected = [
            CheckId::M1,
            CheckId::M2,
            CheckId::M3,
            CheckId::M4,
            CheckId::M5,
            CheckId::M6,
            CheckId::L1,
            CheckId::L2,
            CheckId::L3,
            CheckId::I1,
            CheckId::I2,
            CheckId::I3,
            CheckId::H1,
            CheckId::H2,
            CheckId::T1,
            CheckId::T2,
            CheckId::G1,
            CheckId::G2,
            CheckId::G3,
            CheckId::Geo1,
        ];
        assert_eq!(ids, expected, "ids appear in spec order");

        // The S1-owned checks ran and passed on the valid fixture.
        for id in [
            CheckId::H1,
            CheckId::H2,
            CheckId::I3,
            CheckId::T1,
            CheckId::G1,
        ] {
            let outcome = report.find(id).expect("S1 check present");
            assert_eq!(outcome.status(), CheckStatus::Ran, "{id:?} ran");
            assert_eq!(outcome.result(), Some(CheckResult::Pass), "{id:?} passed");
        }
        // The cross-file checks are honest skips (MS6-S2 flips them).
        for id in [
            CheckId::M5,
            CheckId::L1,
            CheckId::I2,
            CheckId::G2,
            CheckId::Geo1,
        ] {
            let outcome = report.find(id).expect("cross-file check present");
            assert_eq!(outcome.status(), CheckStatus::Skipped, "{id:?} skipped");
            assert_eq!(outcome.detail(), Some("not yet wired"));
        }

        // conformant = "no ran-fail": the partial report is conformant.
        assert!(report.conformant());
    }

    #[test]
    fn skipped_check_never_flips_conformant() {
        use crate::validate::CheckOutcome;
        let checks = vec![
            CheckOutcome::ran_pass(CheckId::H1, DepthClass::MetadataDeep),
            CheckOutcome::skipped(CheckId::M5, DepthClass::MetadataDeep, "not yet wired"),
            CheckOutcome::ran_pass(CheckId::I3, DepthClass::MetadataDeep),
        ];
        let report = ValidationReport::from_outcomes(checks);
        assert!(report.conformant(), "a skip never flips the verdict");
    }

    #[test]
    fn a_single_ran_fail_makes_the_report_non_conformant() {
        use crate::validate::CheckOutcome;
        let checks = vec![
            CheckOutcome::ran_pass(CheckId::H1, DepthClass::MetadataDeep),
            CheckOutcome::ran_fail(CheckId::I3, DepthClass::MetadataDeep, "duplicate"),
            CheckOutcome::skipped(CheckId::M5, DepthClass::MetadataDeep, "not yet wired"),
        ];
        let report = ValidationReport::from_outcomes(checks);
        assert!(
            !report.conformant(),
            "a ran:fail makes the report non-conformant"
        );
    }

    #[test]
    fn check_id_as_str_is_stable() {
        assert_eq!(CheckId::M1.as_str(), "M1");
        assert_eq!(CheckId::Geo1.as_str(), "Geo1");
        assert_eq!(CheckId::G3.as_str(), "G3");
    }
}
