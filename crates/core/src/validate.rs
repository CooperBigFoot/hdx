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
//! ## The §14 checklist (MS6-S1 + MS6-S2)
//!
//! MS6-S1 froze the report wire shape + the per-check rule-function surface and
//! implemented every check whose rule is a **pure function over the already-typed
//! discovery model and is falsifiable in-memory without differently-shaped on-disk
//! bytes** (the MED-2 fold): **H1, H2, I3, T1, G1**, plus the entry-gate **M1, M2, M3,
//! M4** (folded into [`Manifest::from_json`]).
//!
//! MS6-S2 completes the checklist with the checks whose rule needs the **full discovery
//! layer assembled from on-disk bytes**: **L1, L2, L3, I1, I2, M5, M6, T2, G2, G3,
//! Geo1**. Every §14 id now ends up either `ran` (pass/fail) or **honestly `skipped`
//! with a reason** under R3; the report states which ran (spec §14 note). The legs that
//! genuinely need a byte-deep / on-disk-shape-dependent read are honest R3 skips:
//!
//! - **M6 rule (b)** — per-basin axis *regularity* needs the full 1-D `time` array;
//!   v0.1 discovery surfaces only a two-point `[start, end]` extent + sortedness, so the
//!   regularity leg is [`DepthClass::ByteDeep`]-skipped (rule (a), cadence-non-empty,
//!   runs and passes). See [`check_m6`] for the FOLD MED-1 rule verbatim.
//! - **T2** — cross-artifact *full* time-axis identity between the scalar and gridded
//!   dynamic axes needs both full 1-D axes; the cheap leg confirmable from metadata runs,
//!   else the leg is an honest R3 skip (the on-disk negative is MS8).
//!
//! The **on-disk negative matrix** (I2 folder mismatch, T2 cross-artifact axis, G2
//! misaligned-shared-grid, G3 missing georef, L1/L2/L3 layout mutations, Geo1
//! column/partition, M5 file crs-mismatch) is completed in **MS8** — MS6 makes **no**
//! claim of an on-disk negative for those (see the test-module deferral comment).
//!
//! No check decodes a gridded chunk or pixel raster (LOW-3): every check runs over the
//! discovery layer + the 1-D index reads MS3/MS4 already perform.
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

use serde::Serialize;
use tracing::{debug, info, instrument};

use crate::discovery::BasinScalar;
use crate::error::ValidateError;
use crate::field::{Field, Quadrant};
use crate::geoparquet_reader::OutlinesInfo;
use crate::grid::GridInfo;
use crate::gridded_discovery::{BasinGridded, Discovery, discover};
use crate::manifest::Manifest;
use crate::newtypes::{BasinId, Cadence, Crs, GridLabel};
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

impl CheckStatus {
    /// Returns the stable wire string (`"ran"` / `"skipped"`) for the report.
    pub fn as_str(&self) -> &'static str {
        match self {
            CheckStatus::Ran => "ran",
            CheckStatus::Skipped => "skipped",
        }
    }
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

impl CheckResult {
    /// Returns the stable wire string (`"pass"` / `"fail"`) for the report.
    pub fn as_str(&self) -> &'static str {
        match self {
            CheckResult::Pass => "pass",
            CheckResult::Fail => "fail",
        }
    }
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

    /// Maps this [`ValidationReport`] into its serializable [`ValidationReportDto`]
    /// (the pinned wire shape, spec §10/§14).
    ///
    /// The DTO owns the JSON wire shape; this is the single place the inert domain types
    /// are projected onto it. Borrowing — no clones beyond the `&str` views the DTO holds.
    pub fn to_dto(&self) -> ValidationReportDto<'_> {
        ValidationReportDto {
            checks: self.checks.iter().map(CheckOutcomeDto::from_outcome).collect(),
            conformant: self.conformant,
        }
    }

    /// Serializes this [`ValidationReport`] to a compact JSON string (the wire shape).
    ///
    /// # Errors
    ///
    /// | Condition | Error |
    /// |---|---|
    /// | the DTO cannot be serialized (practically unreachable — only stable `&str`s and a bool) | [`serde_json::Error`] |
    pub fn to_json_string(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(&self.to_dto())
    }

    /// Serializes this [`ValidationReport`] to a pretty-printed JSON string (the wire
    /// shape, indented; the form the golden report pins).
    ///
    /// # Errors
    ///
    /// | Condition | Error |
    /// |---|---|
    /// | the DTO cannot be serialized (practically unreachable — only stable `&str`s and a bool) | [`serde_json::Error`] |
    pub fn to_json_pretty(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(&self.to_dto())
    }
}

/// The serializable top-level `validate` report shape. Owns the JSON wire shape.
///
/// Exactly `{checks, conformant}` (spec §10/§14): the per-check outcomes plus the overall
/// verdict. The shape is **inert/agnostic** (spec §1) — a check entry carries only its id,
/// ran/skip status, pass/fail result, R3 depth class, and an opaque detail string; no
/// derived domain field. The inert [`CheckOutcome`] / [`ValidationReport`] gain **no**
/// `serde::Serialize` derive; this describe-local DTO is the single wire-shape surface, the
/// same two-stage discipline `describe` uses (architecture §3.5/§5, R4). The shape is
/// versioned **implicitly by `format_version` only** (the hard cut, spec §0/§11) — there is
/// no separate schema-version field.
#[derive(Debug, Serialize)]
pub struct ValidationReportDto<'a> {
    /// Source: [`ValidationReport::checks`] (every §14 id, in spec order).
    checks: Vec<CheckOutcomeDto<'a>>,
    /// Source: [`ValidationReport::conformant`] ("no check that ran failed", spec §14).
    conformant: bool,
}

/// The serializable per-check outcome shape — exactly `{id, status, result, depth,
/// detail}` (spec §14).
///
/// Mirrors one [`CheckOutcome`] onto the wire: the stable spec id, the ran/skip status,
/// the pass/fail result (`null` for a skipped check), the R3 depth class, and the opaque
/// detail/reason string (`null` when none). This is the machine-readable form of the §14
/// note requirement — the report **clearly reports which checks ran vs were skipped**.
#[derive(Debug, Serialize)]
struct CheckOutcomeDto<'a> {
    /// Source: [`CheckOutcome::id`] via [`CheckId::as_str`] (`"M1"`…`"Geo1"`).
    id: &'a str,
    /// Source: [`CheckOutcome::status`] via [`CheckStatus::as_str`] (`"ran"` / `"skipped"`).
    status: &'a str,
    /// Source: [`CheckOutcome::result`] via [`CheckResult::as_str`] (`"pass"` / `"fail"`),
    /// or `null` for a skipped check.
    result: Option<&'a str>,
    /// Source: [`CheckOutcome::depth`] via [`DepthClass::as_str`]
    /// (`"metadata_deep"` / `"byte_deep"`).
    depth: &'a str,
    /// Source: [`CheckOutcome::detail`] — the opaque fail/skip reason, or `null` when none.
    detail: Option<&'a str>,
}

impl<'a> CheckOutcomeDto<'a> {
    /// Projects one [`CheckOutcome`] onto the wire shape (borrowing its stable strings).
    fn from_outcome(outcome: &'a CheckOutcome) -> Self {
        Self {
            id: outcome.id().as_str(),
            status: outcome.status().as_str(),
            result: outcome.result().map(|r| r.as_str()),
            depth: outcome.depth().as_str(),
            detail: outcome.detail(),
        }
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

// --- The cross-file / cross-basin rule functions (MS6-S2) ----------------------------
//
// Each rule below needs the full discovery layer assembled from on-disk bytes (the
// layout walk + the metadata readers MS3/MS4 already ran). They reference the reserved
// `CoreError` detail vocabulary (`MissingRootRollup`, `BasinIdFolderMismatch`,
// `RaggedSchema`, `GridLabelMismatchAcrossBasins`, `NonMonotonicTime`) in their fail
// messages, but a violated MUST is a recorded fail `CheckOutcome`, never a returned
// `Err` (the report-vs-error split). No rule decodes a gridded chunk or pixel raster
// (LOW-3) — each is `MetadataDeep` unless its rule genuinely needs a byte-deep read,
// in which case that leg is an honest R3 `Skipped`-with-reason (`ByteDeep`).

/// The two root-rollup artifact names (spec §4/§14 L1) — the detail vocabulary.
const SCALAR_STATIC_ARTIFACT: &str = "scalar_static.parquet";
const OUTLINES_ARTIFACT: &str = "outlines.geoparquet";

/// Checks L1 — both root rollups exist at the dataset root (spec §4/§14 L1).
///
/// `scalar_static.parquet` and `outlines.geoparquet` MUST both be present at the root;
/// an absent rollup ⇒ `ran:fail` with a [`CoreError::MissingRootRollup`]-style detail
/// naming the missing artifact. R3: `MetadataDeep` (decided from the layout walk's
/// recorded presence facts — no bytes read).
///
/// [`CoreError::MissingRootRollup`]: crate::error::CoreError::MissingRootRollup
#[instrument(skip(discovery))]
pub fn check_l1(discovery: &Discovery) -> CheckOutcome {
    let rollups = discovery.scalar().root_rollups();
    let missing: Vec<&str> = [
        (rollups.scalar_static_present(), SCALAR_STATIC_ARTIFACT),
        (rollups.outlines_present(), OUTLINES_ARTIFACT),
    ]
    .into_iter()
    .filter_map(|(present, name)| if present { None } else { Some(name) })
    .collect();

    if missing.is_empty() {
        CheckOutcome::ran_pass(CheckId::L1, DepthClass::MetadataDeep)
    } else {
        debug!(?missing, "L1: a root rollup is absent");
        CheckOutcome::ran_fail(
            CheckId::L1,
            DepthClass::MetadataDeep,
            format!("missing root rollup {:?}", missing.join(", ")),
        )
    }
}

/// Checks L2 — each basin carries its required per-basin artifacts (spec §4/§14 L2).
///
/// The required artifacts are **derived from the field set** (not a fixed dataset mode,
/// spec §2 / architecture §3.3):
///
/// - every basin MUST carry a `scalar_dynamic.parquet` (surfaced as a `time`
///   descriptor — a basin with none failed to expose the required artifact);
/// - **iff** the field schema declares `GriddedStatic` fields, every basin MUST expose
///   a `gridded_static/` artifact; **iff** it declares `GriddedDynamic` fields, every
///   basin MUST expose a `gridded_dynamic/` artifact.
///
/// A missing required artifact ⇒ `ran:fail`. The reverse direction — a gridded subtree
/// *present on disk* with **no** gridded fields — needs the raw subtree-directory
/// presence fact (an on-disk-shape mutation), so it is the MS8 negative; the forward
/// direction (the fixture's positive path) runs here. R3: `MetadataDeep`.
#[instrument(skip(discovery))]
pub fn check_l2(discovery: &Discovery) -> CheckOutcome {
    let declares_gridded_static = discovery
        .gridded()
        .gridded_fields()
        .iter()
        .any(|f| f.quadrant() == Quadrant::GriddedStatic);
    let declares_gridded_dynamic = discovery
        .gridded()
        .gridded_fields()
        .iter()
        .any(|f| f.quadrant() == Quadrant::GriddedDynamic);

    // scalar_dynamic leg: a present-and-readable scalar_dynamic surfaces a `time`
    // descriptor; an absent one yields `None` (the gap the L2 MUST forbids).
    for basin in discovery.scalar().per_basin() {
        if basin.time().is_none() {
            debug!(
                basin = basin.basin_id_folder().as_str(),
                "L2: basin missing scalar_dynamic.parquet"
            );
            return CheckOutcome::ran_fail(
                CheckId::L2,
                DepthClass::MetadataDeep,
                format!(
                    "basin {:?} is missing its required scalar_dynamic.parquet",
                    basin.basin_id_folder().as_str()
                ),
            );
        }
    }

    // gridded subtree legs: required iff the field set declares that gridded quadrant.
    for basin in discovery.gridded().per_basin() {
        if declares_gridded_static && basin.static_artifacts().is_empty() {
            return CheckOutcome::ran_fail(
                CheckId::L2,
                DepthClass::MetadataDeep,
                format!(
                    "basin {:?} declares gridded·static fields but exposes no gridded_static artifact",
                    basin.basin_id_folder().as_str()
                ),
            );
        }
        if declares_gridded_dynamic && basin.dynamic_artifacts().is_empty() {
            return CheckOutcome::ran_fail(
                CheckId::L2,
                DepthClass::MetadataDeep,
                format!(
                    "basin {:?} declares gridded·dynamic fields but exposes no gridded_dynamic artifact",
                    basin.basin_id_folder().as_str()
                ),
            );
        }
    }

    CheckOutcome::ran_pass(CheckId::L2, DepthClass::MetadataDeep)
}

/// Checks L3 — no stray / ragged HDX files; an absent field is NaN, never a missing
/// file (spec §5/§14 L3).
///
/// The layout walk already filters dot-cruft / OS scratch
/// ([`is_ignored_entry`](crate::layout::is_ignored_entry)), pre-empting the common
/// stray-file false positive, and discovery already read every present HDX artifact
/// successfully (a structurally bad artifact would have failed discovery as an `Err`).
/// What L3 *also* forbids — a producer encoding a field's absence as a *missing file*
/// rather than a NaN-filled column — is a value-shape mutation only a byte-deep read of
/// the actual cell payloads could detect, which `validate` deliberately never does
/// (LOW-3). v0.1 therefore confirms the **metadata-deep** legs (no stray entries
/// enumerated; every present artifact decoded) and honestly **R3-skips** the byte-deep
/// absence-vs-NaN leg with a reason. R3: `ByteDeep` (the deferred leg).
#[instrument(skip(_discovery))]
pub fn check_l3(_discovery: &Discovery) -> CheckOutcome {
    CheckOutcome::skipped(
        CheckId::L3,
        DepthClass::ByteDeep,
        "the metadata-deep legs hold by construction (the walk ignores dot-cruft and \
         every present artifact decoded during discovery); the absence-is-NaN-not-a- \
         missing-file leg needs a byte-deep read of the cell payloads, which validate \
         never performs (LOW-3) — deferred to the MS8 on-disk matrix",
    )
}

/// Checks I1 — `basin_id` is a real column in every required artifact (spec §3/§14 I1).
///
/// The `basin_id` column MUST be present in `scalar_static`, in **every**
/// `scalar_dynamic`, and in `outlines`:
///
/// - the per-basin `scalar_dynamic` leg uses
///   [`basin_id_in_file().is_some()`](crate::discovery::BasinScalar::basin_id_in_file)
///   (a surfaced value implies the column was read);
/// - the `scalar_static` leg uses the additive
///   [`scalar_static_has_basin_id`](crate::discovery::ScalarDiscovery::scalar_static_has_basin_id)
///   accessor (`None` ⇒ the rollup is absent, an L1 concern, so this leg does not fail);
/// - the `outlines` leg uses MS4's read: the geoparquet reader *requires*
///   `basin_id`/`delineation`/`geometry` and errors otherwise, so a present-and-readable
///   outlines satisfies the column-presence leg (and an absent outlines is an L1 fail,
///   not an I1 fail).
///
/// Any required-and-present artifact lacking the column ⇒ `ran:fail`. R3: `MetadataDeep`.
#[instrument(skip(discovery))]
pub fn check_i1(discovery: &Discovery) -> CheckOutcome {
    // scalar_static leg (when the rollup is present): the column must be there.
    if discovery.scalar().scalar_static_has_basin_id() == Some(false) {
        return CheckOutcome::ran_fail(
            CheckId::I1,
            DepthClass::MetadataDeep,
            "scalar_static.parquet has no basin_id column",
        );
    }

    // Every present scalar_dynamic must carry a basin_id value (column present).
    for basin in discovery.scalar().per_basin() {
        // Only basins that actually have a scalar_dynamic (a `time` descriptor) are
        // subject to I1 here; an absent scalar_dynamic is an L2 concern, not I1.
        if basin.time().is_some() && basin.basin_id_in_file().is_none() {
            return CheckOutcome::ran_fail(
                CheckId::I1,
                DepthClass::MetadataDeep,
                format!(
                    "basin {:?} scalar_dynamic.parquet has no basin_id column",
                    basin.basin_id_folder().as_str()
                ),
            );
        }
    }

    // The outlines leg: a present outlines that read successfully carries basin_id by
    // construction (the reader errors otherwise). An absent outlines is L1, not I1.
    if let Some(outlines) = discovery.gridded().outlines()
        && !outlines.has_basin_id()
    {
        return CheckOutcome::ran_fail(
            CheckId::I1,
            DepthClass::MetadataDeep,
            "outlines.geoparquet has no basin_id column",
        );
    }

    CheckOutcome::ran_pass(CheckId::I1, DepthClass::MetadataDeep)
}

/// Checks I2 — the in-file `basin_id` agrees with its `basin=<id>` folder (spec §3/§14 I2).
///
/// For every basin that surfaced an in-file `basin_id`, that authoritative value MUST
/// equal the locality id parsed from the `basin=<id>` directory; a disagreement ⇒
/// `ran:fail` with a [`CoreError::BasinIdFolderMismatch`]-style detail. A basin with no
/// in-file id (no readable column) is an I1 concern, not an I2 disagreement, so it is
/// skipped here. R3: `MetadataDeep`.
///
/// Input: one entry per basin, pairing the folder id with its `Option<&BasinId>` in-file
/// id (borrowed from the discovery model so the rule stays pure and in-memory-falsifiable
/// — a hand-built `(folder, in_file)` pair falsifies it without on-disk bytes).
///
/// [`CoreError::BasinIdFolderMismatch`]: crate::error::CoreError::BasinIdFolderMismatch
#[instrument(skip(per_basin))]
pub fn check_i2(per_basin: &[(&BasinId, Option<&BasinId>)]) -> CheckOutcome {
    for (folder, in_file) in per_basin {
        let Some(in_file) = in_file else {
            continue;
        };
        if in_file != folder {
            debug!(
                in_file = in_file.as_str(),
                folder = folder.as_str(),
                "I2: in-file basin_id does not match its folder"
            );
            return CheckOutcome::ran_fail(
                CheckId::I2,
                DepthClass::MetadataDeep,
                format!(
                    "basin_id {:?} does not match its partition folder {:?}",
                    in_file.as_str(),
                    folder.as_str()
                ),
            );
        }
    }
    CheckOutcome::ran_pass(CheckId::I2, DepthClass::MetadataDeep)
}

/// Checks M5 — the manifest `crs` matches every georeferenced file's recorded CRS
/// (spec §7/§11/§14 M5).
///
/// The manifest `crs` MUST equal the [`Crs`] recorded on **every** [`GridInfo`] and on
/// the `outlines.geoparquet`; a mismatch ⇒ `ran:fail`. A file whose CRS could not be
/// resolved to a comparable `EPSG:<code>` (recorded raw with an
/// [`CrsSource::RawProjjsonR3`](crate::geoparquet_reader::CrsSource::RawProjjsonR3) flag
/// by MS4) makes that file's M5 leg an honest `skipped`-with-reason, never a silent pass
/// — but on the fixture every file resolves a comparable `EPSG:4326`, so M5 runs. R3:
/// `MetadataDeep`.
#[instrument(skip(manifest_crs, grids, outlines))]
pub fn check_m5(
    manifest_crs: &Crs,
    grids: &[GridInfo],
    outlines: Option<&OutlinesInfo>,
) -> CheckOutcome {
    use crate::geoparquet_reader::CrsSource;

    for grid in grids {
        if grid.crs() != manifest_crs {
            debug!(
                grid_label = grid.grid_label().as_str(),
                manifest = manifest_crs.as_str(),
                file = grid.crs().as_str(),
                "M5: grid CRS differs from the manifest"
            );
            return CheckOutcome::ran_fail(
                CheckId::M5,
                DepthClass::MetadataDeep,
                format!(
                    "grid {:?} CRS {:?} does not match the manifest crs {:?}",
                    grid.grid_label().as_str(),
                    grid.crs().as_str(),
                    manifest_crs.as_str()
                ),
            );
        }
    }

    if let Some(outlines) = outlines {
        match outlines.crs_source() {
            // The raw-PROJJSON file cannot be compared to the manifest cheaply: an
            // honest R3 skip-with-reason rather than a silent pass (the on-disk
            // crs-mismatch negative is MS8).
            CrsSource::RawProjjsonR3 => {
                return CheckOutcome::skipped(
                    CheckId::M5,
                    DepthClass::MetadataDeep,
                    "outlines.geoparquet CRS resolved to a raw PROJJSON (no comparable \
                     EPSG id); byte-deep CRS comparison deferred (R3)",
                );
            }
            CrsSource::EpsgFromProjjsonId => {
                if outlines.crs() != manifest_crs {
                    return CheckOutcome::ran_fail(
                        CheckId::M5,
                        DepthClass::MetadataDeep,
                        format!(
                            "outlines.geoparquet CRS {:?} does not match the manifest crs {:?}",
                            outlines.crs().as_str(),
                            manifest_crs.as_str()
                        ),
                    );
                }
            }
        }
    }

    CheckOutcome::ran_pass(CheckId::M5, DepthClass::MetadataDeep)
}

/// Checks M6 — the cadence convention vs the realized `time` axes (spec §6.4/§14 M6).
///
/// **FOLD MED-1 — the load-bearing M6 rule.** HDX **parses no cadence semantics**
/// (spec §1/§6.4) and §6.1 explicitly permits **ragged per-basin time extents**. So M6
/// is implemented as EXACTLY two rules and nothing more:
///
/// - **rule (a)** — `cadence` is a **non-empty string** (this is also M4; M6 references
///   it, it does not re-own it). On the fixture this **runs and passes**.
/// - **rule (b)** — each basin's realized `time` axis is **INTERNALLY regular**
///   (uniformly spaced within that basin — the §6.2 consequence that gaps are NaN-filled,
///   so a conformant per-basin axis has a constant interior step).
///
/// **The documented limit: HDX verifies axis REGULARITY, not that the spacing matches
/// the cadence *word*.** M6 **never** interprets `"daily"` as a 1-day step (that would
/// be the semantic interpretation HDX must avoid), and M6 asserts **no cross-basin
/// time-extent equality** — a merely-different cross-basin step is **not** a failure
/// (§6.1), and if cross-basin step consistency is reported at all it is the **first R3
/// skip-with-reason**, never a hard fail.
///
/// **v0.1 outcome.** Rule (b)'s only cheap signal in v0.1 discovery is a two-point
/// `[start, end]` extent + a `sorted_ascending` flag — from which a constant *interior*
/// step is **not** derivable. So rule (b) is honestly **R3 `Skipped`-with-reason** and
/// classified [`DepthClass::ByteDeep`] (it needs the full 1-D `time` array). Because a
/// `Skipped` leg is **not** a fail, and rule (a) passes, the dataset stays conformant.
/// M6's single [`CheckOutcome`] records this as `Skipped` (the dominant honest status),
/// with a detail naming the regularity leg as the deferred one and confirming rule (a)
/// (cadence non-empty) passed. R3: `ByteDeep` (the regularity leg dominates the depth).
#[instrument(skip(cadence))]
pub fn check_m6(cadence: &Cadence) -> CheckOutcome {
    // Rule (a): cadence non-empty (references M4). A violated rule (a) is a real fail.
    if cadence.as_str().is_empty() {
        return CheckOutcome::ran_fail(
            CheckId::M6,
            DepthClass::MetadataDeep,
            "cadence must be a non-empty string (M6 rule (a))",
        );
    }

    // Rule (b): per-basin axis regularity — honest R3 skip in v0.1. The reason names the
    // regularity leg (NOT the cadence word) and records that rule (a) passed.
    CheckOutcome::skipped(
        CheckId::M6,
        DepthClass::ByteDeep,
        "rule (a) cadence-non-empty passed; rule (b) per-basin axis REGULARITY needs the \
         full 1-D time array, but v0.1 discovery surfaces only [start,end] + sortedness \
         — byte-deep axis-regularity verification deferred (no cadence-word \
         interpretation, no cross-basin step equality asserted)",
    )
}

/// Checks T2 — within each basin the scalar and gridded·dynamic axes share the identical
/// `time` axis; gaps are NaN-filled (spec §6.2/§14 T2).
///
/// A basin's `scalar_dynamic` `time` axis and every `gridded_dynamic` artifact MUST be
/// the **identical** time axis (§6.2). v0.1 discovery surfaces the scalar `[start, end]`
/// extent and the Zarr per-grid geometry, but **not** the Zarr `time` coordinate axis as
/// a comparable 1-D array on the model — so the cross-artifact full-axis identity is the
/// genuinely on-disk-shape-dependent leg reserved for **MS8**. v0.1 therefore reports T2
/// as an honest R3 `Skipped`-with-reason. R3: `ByteDeep`.
#[instrument(skip(_discovery))]
pub fn check_t2(_discovery: &Discovery) -> CheckOutcome {
    CheckOutcome::skipped(
        CheckId::T2,
        DepthClass::ByteDeep,
        "intra-basin scalar-vs-gridded full time-axis identity needs both 1-D axes; \
         v0.1 discovery surfaces the scalar [start,end] extent but not a comparable \
         gridded time axis on the model — byte-deep cross-artifact axis identity \
         deferred to the MS8 on-disk matrix",
    )
}

/// Checks G2 — a grid label shared across the COG and Zarr subtrees implies cell-for-cell
/// alignment (spec §8/§14 G2).
///
/// When a label appears in **both** a basin's `gridded_static` (COG) and
/// `gridded_dynamic` (Zarr) subtrees, the two [`GridInfo`]s MUST **coincide** in extent,
/// resolution, and pixel dimensions (spec §8 — one artifact = one grid, a shared label
/// signals alignment). A shared-but-misaligned label ⇒ `ran:fail`. **Its positive path
/// is exercised on the MS2 valid fixture** (critique H-1): both subtrees report `era5`
/// and coincide at `10.0/50.0/11.5/48.0`, 6×8 ⇒ `ran:pass`. A dataset with no shared
/// label has nothing to enforce and trivially passes. R3: `MetadataDeep` (the geometry
/// was already metadata-decoded by MS4 — no chunk read here).
#[instrument(skip(per_basin))]
pub fn check_g2(per_basin: &[&BasinGridded]) -> CheckOutcome {
    for basin in per_basin {
        for static_artifact in basin.static_artifacts() {
            let label = static_artifact.grid_label();
            // The matching Zarr artifact for the same label, if any.
            let Some(dynamic_artifact) = basin
                .dynamic_artifacts()
                .iter()
                .find(|d| d.grid_label() == label)
            else {
                continue;
            };
            let cog = static_artifact.grid_info();
            let zarr = dynamic_artifact.grid_info();
            let aligned = cog.extent() == zarr.extent()
                && cog.resolution() == zarr.resolution()
                && cog.width() == zarr.width()
                && cog.height() == zarr.height();
            if !aligned {
                debug!(
                    label = label.as_str(),
                    basin = basin.basin_id_folder().as_str(),
                    "G2: shared label does not coincide across subtrees"
                );
                return CheckOutcome::ran_fail(
                    CheckId::G2,
                    DepthClass::MetadataDeep,
                    format!(
                        "shared grid label {:?} in basin {:?} is not cell-for-cell aligned \
                         across the gridded_static (COG) and gridded_dynamic (Zarr) subtrees",
                        label.as_str(),
                        basin.basin_id_folder().as_str()
                    ),
                );
            }
        }
    }
    CheckOutcome::ran_pass(CheckId::G2, DepthClass::MetadataDeep)
}

/// Checks G3 — every grid carries resolvable CF / GeoTIFF georeferencing (spec §7/§14 G3).
///
/// Each gridded artifact MUST be georeferenced (Zarr via a CF `grid_mapping`, COG via the
/// standard GeoTIFF tags). MS4's readers already error
/// [`MissingGridGeoref`](crate::error::CoreError::MissingGridGeoref) when a present
/// artifact has none, so a discovered [`GridInfo`] carrying a recorded [`Crs`] satisfies
/// G3 (its georef was resolved to record the extent + CRS). A dataset with no grids has
/// nothing to enforce and trivially passes. R3: `MetadataDeep`.
#[instrument(skip(grids))]
pub fn check_g3(grids: &[GridInfo]) -> CheckOutcome {
    // A present GridInfo carries a recorded CRS by construction (the reader could not
    // have built it without resolving georef); G3 is the explicit check that every grid
    // surfaced one. An empty (or accidentally blank) CRS string is the falsifiable form.
    for grid in grids {
        if grid.crs().as_str().is_empty() {
            return CheckOutcome::ran_fail(
                CheckId::G3,
                DepthClass::MetadataDeep,
                format!(
                    "grid {:?} carries no resolvable georeferencing (empty CRS)",
                    grid.grid_label().as_str()
                ),
            );
        }
    }
    CheckOutcome::ran_pass(CheckId::G3, DepthClass::MetadataDeep)
}

/// Checks Geo1 — `outlines.geoparquet` has rows `(basin_id, delineation, geometry)`, the
/// label column is `delineation`, and it is **not** partitioned by delineation
/// (spec §9/§14 Geo1).
///
/// MS4's geoparquet reader already enforces the three required columns (erroring
/// [`MissingGeometryColumn`](crate::error::CoreError::MissingGeometryColumn) otherwise)
/// and reads a **single root file** (recording
/// [`partitioned_by_delineation`](crate::geoparquet_reader::OutlinesInfo::partitioned_by_delineation)
/// as `false`), so a present-and-read outlines satisfies Geo1 ⇒ `ran:pass`. An **absent**
/// outlines is an [`L1`](CheckId::L1) fail, **not** a Geo1 fail — so Geo1 is honestly
/// `skipped`-with-reason when there is no outlines to check. R3: `MetadataDeep`.
#[instrument(skip(outlines))]
pub fn check_geo1(outlines: Option<&OutlinesInfo>) -> CheckOutcome {
    let Some(outlines) = outlines else {
        return CheckOutcome::skipped(
            CheckId::Geo1,
            DepthClass::MetadataDeep,
            "no outlines.geoparquet to check (its absence is an L1 failure, not a Geo1 \
             one)",
        );
    };

    if !outlines.has_basin_id() || !outlines.has_delineation() || !outlines.has_geometry() {
        return CheckOutcome::ran_fail(
            CheckId::Geo1,
            DepthClass::MetadataDeep,
            "outlines.geoparquet is missing one of (basin_id, delineation, geometry)",
        );
    }
    if outlines.partitioned_by_delineation() {
        return CheckOutcome::ran_fail(
            CheckId::Geo1,
            DepthClass::MetadataDeep,
            "outlines.geoparquet is partitioned by delineation (it must be a single root \
             file with a delineation column)",
        );
    }
    CheckOutcome::ran_pass(CheckId::Geo1, DepthClass::MetadataDeep)
}

// --- The verb -----------------------------------------------------------------------

/// Validates a dataset against the §14 `MUST` checklist, returning a [`ValidationReport`]
/// (spec §10/§14).
///
/// Runs the §0 entry gate first (read `manifest.json` → [`Manifest::from_json`] hard cut →
/// [`discover`]), then runs the full §14 checklist over the assembled discovery model.
/// The report lists all **20** §14 ids, each `ran` (pass/fail) or honestly `skipped` with
/// a reason: M1–M4 via the entry gate; H1, H2, I3, T1, G1 in-memory; L1, L2, I1, I2, M5,
/// G2, G3 cross-file; and the byte-deep / on-disk-shape-dependent legs (L3, M6 rule (b),
/// T2, and Geo1 when outlines is absent) as honest R3 `Skipped`-with-reason.
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
    let manifest = Manifest::from_json(&manifest_json).map_err(ValidateError::Manifest)?;
    debug!("manifest boundary-parse passed (M1/M2 hard cut + M3/M4 cleared)");

    // Stage 3 — discovery: only now is any other file in the dataset read.
    let discovery = discover(path).map_err(ValidateError::Discovery)?;
    debug!("discovery complete");

    // Stage 4 — build the report by running the §14 rules over the assembled model. The
    // manifest's crs/cadence feed M5/M6; a violated MUST is a recorded fail outcome.
    let report = build_report(&discovery, &manifest);
    info!(
        conformant = report.conformant(),
        checks = report.checks().len(),
        "validated dataset"
    );
    Ok(report)
}

/// Validates a dataset and serializes the [`ValidationReport`] to its stable JSON string
/// (the wire shape the CLI (MS7) and the PyO3 binding (MS9) consume, spec §10/§14).
///
/// A thin wrapper over [`validate`] + [`ValidationReport::to_json_string`]; the same §0
/// entry discipline and report-vs-error split apply. A **violated `MUST`** is carried in
/// the JSON as a `result:"fail"` outcome with `conformant:false`, never raised as an
/// `Err` — only a structural/entry failure (or the §0 hard cut) is an `Err`.
///
/// # Errors
///
/// | Condition | Error |
/// |---|---|
/// | [`validate`] fails (unreadable/malformed manifest, the §0 hard cut, or discovery) | the propagated [`ValidateError`] |
/// | the assembled [`ValidationReport`] cannot be serialized (practically unreachable — only stable enum strings + a bool) | [`ValidateError::Serialize`] |
#[instrument(fields(path = %path.as_ref().display()))]
pub fn validate_json(path: impl AsRef<Path>) -> Result<String, ValidateError> {
    let report = validate(path)?;
    report
        .to_json_string()
        .map_err(|err| ValidateError::Serialize {
            detail: err.to_string(),
        })
}

/// Assembles the report by running every §14 rule over the discovery model + manifest
/// (spec §14).
///
/// Lists all **20** §14 ids in spec order. M1–M4 are `ran:pass` (the entry gate already
/// cleared them — a non-conformant manifest returns an `Err` before this runs). The
/// remaining checks run their real rules over the assembled model: the in-memory checks
/// (H1, H2, I3, T1, G1) and the cross-file checks (L1, L2, I1, I2, M5, G2, G3) `ran`
/// (pass/fail), and the genuinely byte-deep / on-disk-shape-dependent legs (L3, M6 rule
/// (b), T2, and Geo1 when outlines is absent) are honest R3 `Skipped`-with-reason. Pure:
/// no IO (discovery already read every byte it will read).
fn build_report(discovery: &Discovery, manifest: &Manifest) -> ValidationReport {
    // The in-memory checks (MS6-S1), computed from the discovery accessors.
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

    // The cross-file / cross-basin checks (MS6-S2).
    let l1 = check_l1(discovery);
    let l2 = check_l2(discovery);
    let l3 = check_l3(discovery);
    let i1 = check_i1(discovery);

    let i2_pairs: Vec<(&BasinId, Option<&BasinId>)> = discovery
        .scalar()
        .per_basin()
        .iter()
        .map(|b| (b.basin_id_folder(), b.basin_id_in_file()))
        .collect();
    let i2 = check_i2(&i2_pairs);

    let m5 = check_m5(manifest.crs(), discovery.grids(), discovery.gridded().outlines());
    let m6 = check_m6(manifest.cadence());
    let t2 = check_t2(discovery);

    let per_basin_gridded: Vec<&BasinGridded> = discovery.gridded().per_basin().iter().collect();
    let g2 = check_g2(&per_basin_gridded);
    let g3 = check_g3(discovery.grids());
    let geo1 = check_geo1(discovery.gridded().outlines());

    let checks: Vec<CheckOutcome> = ALL_CHECK_IDS
        .iter()
        .map(|&id| match id {
            // M1–M4 are folded into the entry gate: reaching this point means the manifest
            // parsed, so each cleared its boundary check (a violation would have returned
            // `Err` before discovery). Entry-gate checks are `MetadataDeep`.
            // This `ran_pass` is the already-cleared-at-the-entry-gate convention, NOT a
            // second enforcement site — the only M1–M4 enforcement is `Manifest::from_json`
            // (the early `?` at validate.rs:1275); this arm is never reached on a violation.
            CheckId::M1 | CheckId::M2 | CheckId::M3 | CheckId::M4 => {
                CheckOutcome::ran_pass(id, DepthClass::MetadataDeep)
            }
            CheckId::H1 => h1.clone(),
            CheckId::H2 => h2.clone(),
            CheckId::I3 => i3.clone(),
            CheckId::T1 => t1.clone(),
            CheckId::G1 => g1.clone(),
            CheckId::L1 => l1.clone(),
            CheckId::L2 => l2.clone(),
            CheckId::L3 => l3.clone(),
            CheckId::I1 => i1.clone(),
            CheckId::I2 => i2.clone(),
            CheckId::M5 => m5.clone(),
            CheckId::M6 => m6.clone(),
            CheckId::T2 => t2.clone(),
            CheckId::G2 => g2.clone(),
            CheckId::G3 => g3.clone(),
            CheckId::Geo1 => geo1.clone(),
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
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};

    use crate::error::{CoreError, ValidateError};
    use crate::field::{Dtype, Field, Quadrant, Units};
    use crate::grid::{GridExtent, GridInfo, GridResolution};
    use crate::manifest::Manifest;
    use crate::newtypes::{BasinId, Cadence, Crs, GridLabel};
    use crate::scalar_reader::TimeColumn;

    use serde_json::Value;

    use crate::validate::{
        CheckId, CheckResult, CheckStatus, DepthClass, ValidationReport, check_g1, check_h1,
        check_h2, check_i3, check_m5, check_m6, check_t1, validate, validate_json,
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

    // --- M3 / M4 on-disk entry-gate negatives (Bucket-A, MS8-S2) ----------------------
    //
    // These exercise the committed conformance fixtures `invalid/extra-manifest-field/`
    // (M3: a 7th manifest field) and `invalid/empty-cadence/` (M4: an empty cadence),
    // each one surgical mutation off the valid baseline (LOW-2; the generator's
    // `assert_differs_in_exactly_one_way` proves the one-mutation invariant at
    // generation time). Their negative is an ENTRY-GATE `Err`, NOT a `conformant:false`
    // report: `validate` reads `manifest.json` then `Manifest::from_json` (the early
    // `?`), whose M3/M4 boundary checks fire BEFORE `discover` is ever called — so the
    // verb returns `Err(ValidateError::Manifest(..))` and `build_report` (where M1–M4
    // are listed `ran:pass` by the entry-gate convention) is never reached. The CLI
    // (MS7) maps this `Err` to a distinct exit code from a `conformant:false` verdict.

    #[test]
    fn m3_extra_field_fixture_errs_at_entry_gate() {
        // The on-disk `extra-manifest-field` fixture adds one derivable key
        // (`content_hash`) to the six-field floor (spec §0/§11). M3 rejects it at the
        // entry gate — an `Err(Manifest(ExtraManifestField { field: "content_hash" }))`,
        // never a report. (The 7th-field form is the M3 "too-many" direction; the
        // "too-few" direction is covered by the in-memory missing-field test.)
        match validate(conformance("invalid/extra-manifest-field")) {
            Err(ValidateError::Manifest(CoreError::ExtraManifestField { field })) => {
                assert_eq!(field, "content_hash", "M3 names the offending extra key");
            }
            other => panic!("expected Err(Manifest(ExtraManifestField)) at the entry gate, got {other:?}"),
        }
    }

    #[test]
    fn m4_empty_cadence_fixture_errs_at_entry_gate() {
        // The on-disk `empty-cadence` fixture sets `cadence: ""` (spec §6.4/§11). M4
        // requires `crs`/`cadence` to be non-empty, so the entry gate returns
        // `Err(Manifest(EmptyCadence))` BEFORE discovery.
        //
        // THIS PINS M4, NOT M6 (load-bearing). `check_m6` rule (a) would `ran:fail` on
        // an empty cadence, but the M4 boundary check in `Manifest::from_json` rejects
        // the empty cadence FIRST — the early `?` in `validate` returns before
        // `build_report` (and thus before `check_m6`) ever runs. M6 rule (a)'s fail leg
        // is therefore unreachable-by-construction on the validate path: an empty-cadence
        // tree is an M4 entry-gate `Err`, so no spurious M6 empty-cadence negative exists.
        match validate(conformance("invalid/empty-cadence")) {
            Err(ValidateError::Manifest(CoreError::EmptyCadence)) => {}
            other => panic!("expected Err(Manifest(EmptyCadence)) at the entry gate (M4, not M6), got {other:?}"),
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

        // Every applicable check ran and passed (or is an honest skip) on the valid
        // fixture — the in-memory and cross-file checks all reach a real outcome now.
        for id in [
            CheckId::H1,
            CheckId::H2,
            CheckId::I3,
            CheckId::T1,
            CheckId::G1,
            CheckId::L1,
            CheckId::L2,
            CheckId::I1,
            CheckId::I2,
            CheckId::M5,
            CheckId::G2,
            CheckId::G3,
            CheckId::Geo1,
        ] {
            let outcome = report.find(id).expect("check present");
            assert_eq!(outcome.status(), CheckStatus::Ran, "{id:?} ran");
            assert_eq!(outcome.result(), Some(CheckResult::Pass), "{id:?} passed");
        }

        // conformant = "no ran-fail": the full report is conformant.
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

    // --- MS6-S2: cross-file checks + the three milestone verdicts ---------------------
    //
    // MS8 DEFERRAL STATEMENT (FOLD MED-2). The genuinely on-disk-shape-dependent
    // negatives are reserved for the MS8 invalid family + golden regression matrix, and
    // MS6 makes NO claim of an on-disk negative for any of them:
    //
    //   - I2  — an on-disk folder/in-file `basin_id` mismatch tree;
    //   - T2  — an on-disk scalar-vs-gridded cross-artifact time-axis mismatch;
    //   - G2  — an on-disk shared-but-misaligned grid label tree;
    //   - G3  — an on-disk gridded artifact missing its georeferencing;
    //   - L1/L2/L3 — on-disk layout mutations (stray/ragged files, a missing per-basin
    //                artifact, a gridded subtree present with no gridded fields);
    //   - Geo1 — an on-disk outlines missing a required column / partitioned by
    //            delineation;
    //   - M5  — an on-disk file whose CRS differs from the manifest's.
    //
    // MS6 proves the POSITIVE paths on the valid fixture and the in-memory-falsifiable
    // legs (M5 crs-mismatch, I2 folder-mismatch, M6 cadence) over the typed model.

    /// The valid fixture's manifest crs/cadence, parsed (for the M5/M6 in-memory legs).
    fn valid_manifest() -> Manifest {
        let json = std::fs::read_to_string(conformance("valid/minimal/manifest.json"))
            .expect("the valid fixture manifest must read");
        Manifest::from_json(&json).expect("the valid fixture manifest must parse")
    }

    /// Builds a `GridInfo` for the fixture grid with the given CRS (M5 in-memory leg).
    fn grid_with_crs(crs: &str) -> GridInfo {
        let extent = GridExtent::from_edge_origin(10.0, 50.0, 0.25, 6, 8);
        GridInfo::new(
            GridLabel::new("era5"),
            extent,
            GridResolution::new(0.25, -0.25),
            6,
            8,
            Crs::new(crs),
        )
    }

    // --- The milestone POSITIVE verdict: conformant:true on the valid fixture ---------

    #[test]
    fn valid_fixture_is_conformant_with_no_ran_fail() {
        let report = validate(conformance("valid/minimal")).expect("the valid fixture validates");

        // The milestone positive proof: conformant, and NO check is ran:fail (every
        // applicable check is ran:pass or honestly skipped with a reason).
        assert!(report.conformant(), "the valid fixture must be conformant");
        for outcome in report.checks() {
            assert_ne!(
                outcome.result(),
                Some(CheckResult::Fail),
                "{:?} must not be ran:fail on the valid fixture",
                outcome.id()
            );
            // Every skip carries a non-empty reason (the §14-note honesty requirement).
            if outcome.status() == CheckStatus::Skipped {
                assert!(
                    outcome.detail().is_some_and(|d| !d.is_empty()),
                    "{:?} skip must carry a reason",
                    outcome.id()
                );
            }
        }

        // The report lists every §14 id (the full closed set).
        assert_eq!(report.checks().len(), 20, "the report lists every §14 id");
    }

    // --- G2 positive path fired (FOLD critique H-1) -----------------------------------

    #[test]
    fn g2_positive_path_fires_on_the_shared_aligned_era5_label() {
        let report = validate(conformance("valid/minimal")).expect("the valid fixture validates");

        // G2 ran and passed: the shared `era5` label's COG + Zarr extents coincided.
        let g2 = report.find(CheckId::G2).expect("G2 present");
        assert_eq!(g2.status(), CheckStatus::Ran, "G2 ran");
        assert_eq!(g2.result(), Some(CheckResult::Pass), "G2 passed");

        // Prove (via the model) that the COG + Zarr era5 extents coincided at the
        // byte-true 10.0/50.0/11.5/48.0 — G2's positive precondition.
        let discovery = crate::gridded_discovery::discover(conformance("valid/minimal"))
            .expect("the valid fixture discovers");
        let basin0001 = discovery
            .gridded()
            .per_basin()
            .iter()
            .find(|b| b.basin_id_folder().as_str() == "0001")
            .expect("basin 0001 present");
        let cog = basin0001.static_artifacts()[0].grid_info();
        let zarr = basin0001.dynamic_artifacts()[0].grid_info();
        assert_eq!(cog.extent(), zarr.extent(), "COG and Zarr era5 extents coincide");
        assert_eq!(cog.extent().west(), 10.0);
        assert_eq!(cog.extent().north(), 50.0);
        assert_eq!(cog.extent().east(), 11.5);
        assert_eq!(cog.extent().south(), 48.0);
    }

    // --- conformant:false on wrong-format-version (entry gate, FOLD H-3) --------------

    #[test]
    fn wrong_format_version_never_reports_conformant_true() {
        // The §0 hard cut wins before discovery (matching `describe`): the verb returns
        // Err(Manifest(UnknownFormatVersion)), so the wrong-version tree never produces a
        // conformant:true report. The CLI (MS7) maps this Err to exit 2.
        match validate(conformance("invalid/wrong-format-version")) {
            Err(ValidateError::Manifest(CoreError::UnknownFormatVersion { found })) => {
                assert_eq!(found, "0.2");
            }
            other => panic!("expected Manifest(UnknownFormatVersion), got {other:?}"),
        }
    }

    #[test]
    fn m2_fail_outcome_form_proven_by_a_hand_built_wrong_version_manifest() {
        // The M2 fail-outcome FORM (a conformant:false consequence) is proven by feeding
        // a hand-built wrong-version manifest through the M2 rule (Manifest::from_json):
        // it rejects "0.2" at the boundary, the §0 hard cut. The on-disk negative through
        // the verb is the Err above; the exhaustive on-disk matrix is MS8.
        let wrong = r#"{
            "format_version": "0.2",
            "name": "ds",
            "created_at": "2026-06-01T00:00:00Z",
            "producer_version": "p",
            "crs": "EPSG:4326",
            "cadence": "daily"
        }"#;
        assert!(matches!(
            Manifest::from_json(wrong),
            Err(CoreError::UnknownFormatVersion { .. })
        ));
    }

    // --- conformant:false on missing-root-rollup (L1, FOLD H-3) -----------------------

    #[test]
    fn missing_root_rollup_pins_exactly_l1_and_is_non_conformant() {
        let report = validate(conformance("invalid/missing-root-rollup"))
            .expect("missing-root-rollup discovers (the gap is a check fail, not an Err)");

        // L1 ran and FAILED (the absent outlines.geoparquet).
        let l1 = report.find(CheckId::L1).expect("L1 present");
        assert_eq!(l1.status(), CheckStatus::Ran, "L1 ran");
        assert_eq!(l1.result(), Some(CheckResult::Fail), "L1 failed");
        assert!(
            l1.detail().is_some_and(|d| d.contains("outlines.geoparquet")),
            "L1 detail names the missing outlines rollup"
        );

        // Non-conformant (fail-closed on the one violated MUST that ran).
        assert!(!report.conformant(), "missing rollup ⇒ non-conformant");

        // ONE-VIOLATION DISCIPLINE: EXACTLY L1 fails; every other check is ran:pass or an
        // honest skip (Geo1 honestly skips because the outlines it would check is absent).
        for outcome in report.checks() {
            if outcome.id() == CheckId::L1 {
                continue;
            }
            assert_ne!(
                outcome.result(),
                Some(CheckResult::Fail),
                "only L1 may fail; {:?} also failed",
                outcome.id()
            );
        }

        // Geo1 is an honest skip here (no outlines to check) — its absence is L1's job.
        let geo1 = report.find(CheckId::Geo1).expect("Geo1 present");
        assert_eq!(geo1.status(), CheckStatus::Skipped, "Geo1 honestly skips");
    }

    // --- M6 rule (FOLD MED-1) ---------------------------------------------------------

    #[test]
    fn m6_on_valid_fixture_is_not_a_fail_and_names_the_regularity_leg() {
        let report = validate(conformance("valid/minimal")).expect("the valid fixture validates");
        let m6 = report.find(CheckId::M6).expect("M6 present");

        // M6 is NOT ran:fail (a skip never flips conformant; rule (a) passed).
        assert_ne!(m6.result(), Some(CheckResult::Fail), "M6 must not fail");

        let detail = m6.detail().expect("M6 carries a reason").to_lowercase();
        // The detail names axis REGULARITY (rule (b)) as the R3-skipped leg, confirms
        // rule (a) (cadence non-empty) passed, and does NOT reference the cadence word
        // ("daily") or a cross-basin step.
        assert!(detail.contains("regularity"), "names the regularity leg");
        assert!(
            detail.contains("rule (a)") && detail.contains("cadence-non-empty"),
            "confirms rule (a) cadence-non-empty passed"
        );
        assert!(
            !detail.contains("daily"),
            "M6 must not interpret the cadence word"
        );
        assert!(
            !detail.contains("cross-basin step equality") || detail.contains("no cross-basin"),
            "M6 asserts no cross-basin step equality"
        );
        assert_eq!(m6.depth(), DepthClass::ByteDeep, "regularity leg is byte-deep");
    }

    #[test]
    fn m6_never_fails_for_ragged_extents() {
        // §6.1: ragged per-basin extents must NOT fail M6. With a non-empty cadence M6
        // only skips the regularity leg (it never inspects extents for a cross-basin
        // step). A hand-built non-empty cadence ⇒ M6 is a skip, never a fail.
        let outcome = check_m6(&Cadence::new("daily"));
        assert_ne!(
            outcome.result(),
            Some(CheckResult::Fail),
            "ragged extents never fail M6"
        );
        assert_eq!(outcome.status(), CheckStatus::Skipped, "rule (b) honest R3 skip");

        // An empty cadence DOES fail rule (a) (this is the only M6 fail form).
        let empty = check_m6(&Cadence::new(""));
        assert_eq!(empty.result(), Some(CheckResult::Fail), "empty cadence fails M6 rule (a)");
    }

    // --- M5 in-memory falsifiable leg -------------------------------------------------

    #[test]
    fn m5_negative_on_crs_mismatch_positive_on_match() {
        let manifest = valid_manifest();
        assert_eq!(manifest.crs().as_str(), "EPSG:4326");

        // Mismatch: a hand-built grid in EPSG:3857 vs the manifest's EPSG:4326 ⇒ fail.
        let mismatched = [grid_with_crs("EPSG:3857")];
        let outcome = check_m5(manifest.crs(), &mismatched, None);
        assert_eq!(outcome.status(), CheckStatus::Ran, "M5 ran");
        assert_eq!(outcome.result(), Some(CheckResult::Fail), "M5 fails on mismatch");

        // Match: a grid in EPSG:4326 ⇒ pass.
        let matching = [grid_with_crs("EPSG:4326")];
        let outcome = check_m5(manifest.crs(), &matching, None);
        assert_eq!(outcome.result(), Some(CheckResult::Pass), "M5 passes on match");
    }

    // --- I2 in-memory falsifiable leg -------------------------------------------------

    #[test]
    fn i2_negative_on_folder_mismatch_positive_on_match() {
        let folder = BasinId::new("0001");
        let wrong = BasinId::new("9999");

        // folder=0001, in_file=9999 ⇒ ran:fail.
        let mismatch = vec![(&folder, Some(&wrong))];
        let outcome = crate::validate::check_i2(&mismatch);
        assert_eq!(outcome.status(), CheckStatus::Ran, "I2 ran");
        assert_eq!(outcome.result(), Some(CheckResult::Fail), "I2 fails on mismatch");

        // folder=0001, in_file=0001 ⇒ ran:pass.
        let matching = BasinId::new("0001");
        let agree = vec![(&folder, Some(&matching))];
        assert_eq!(
            crate::validate::check_i2(&agree).result(),
            Some(CheckResult::Pass),
            "I2 passes when the ids agree"
        );

        // A basin with no in-file id is an I1 concern, not an I2 fail.
        let no_in_file: Vec<(&BasinId, Option<&BasinId>)> = vec![(&folder, None)];
        assert_eq!(
            crate::validate::check_i2(&no_in_file).result(),
            Some(CheckResult::Pass),
            "a missing in-file id does not fail I2 (it is I1's concern)"
        );
    }

    // --- Companion-mask / {source}_{variable} ordinariness ----------------------------

    #[test]
    fn validate_treats_companion_mask_fields_as_ordinary() {
        // H1/G1 apply no suffix/prefix special-casing: era5_precipitation and
        // era5_precipitation_was_filled are ordinary gridded fields. The valid fixture
        // carries both and validates conformant:true with H1 and G1 ran:pass — which
        // already implies no name magic; this pins it explicitly.
        let discovery = crate::gridded_discovery::discover(conformance("valid/minimal"))
            .expect("the valid fixture discovers");
        let names: Vec<&str> = discovery
            .gridded()
            .gridded_fields()
            .iter()
            .map(|f| f.name().as_str())
            .collect();
        assert!(
            names.contains(&"era5_precipitation") && names.contains(&"era5_precipitation_was_filled"),
            "both the {{source}}_{{variable}} field and its companion mask are present, verbatim"
        );

        let report = validate(conformance("valid/minimal")).expect("validates");
        assert_eq!(
            report.find(CheckId::H1).and_then(|c| c.result()),
            Some(CheckResult::Pass),
            "H1 treats the companion-mask field as ordinary"
        );
        assert_eq!(
            report.find(CheckId::G1).and_then(|c| c.result()),
            Some(CheckResult::Pass),
            "G1 treats the companion-mask field as ordinary (no special-casing)"
        );
    }

    // --- MS6-S3: the report wire-shape lock (validate.schema.json + golden snapshot) ---

    /// Resolves a path under the repository-root `schemas/` directory.
    ///
    /// `CARGO_MANIFEST_DIR` is `crates/core`; the committed schemas live two levels up.
    fn schema_path(rel: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../schemas")
            .join(rel)
    }

    /// Loads and compiles the committed `schemas/validate.schema.json`.
    ///
    /// Uses the test-only `jsonschema` dev-dependency (never shipped in `hdx-core`).
    fn validate_validator() -> jsonschema::Validator {
        let path = schema_path("validate.schema.json");
        let raw = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let schema: Value =
            serde_json::from_str(&raw).expect("validate.schema.json must be valid JSON");
        jsonschema::validator_for(&schema)
            .expect("validate.schema.json must compile as a JSON Schema")
    }

    /// Reads the committed golden validate report as a parsed `Value`.
    ///
    /// Regeneration workflow (when the report shape legitimately changes — a
    /// `format_version` bump only): run `validate(conformance("valid/minimal"))`,
    /// pretty-print it (`ValidationReport::to_json_pretty`), and overwrite
    /// `conformance/goldens/valid-minimal.validate.json`. See `conformance/README.md`.
    /// The golden lives OUTSIDE the gitignored fixture trees so `regenerate.sh` never
    /// clobbers it.
    fn golden_value() -> Value {
        let path = conformance("goldens/valid-minimal.validate.json");
        let raw = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        serde_json::from_str(&raw).expect("the golden must be valid JSON")
    }

    /// R4 schema test (jsonschema dev-dep). The committed golden validate report of the
    /// MS2 valid fixture **validates** against the committed `validate.schema.json`,
    /// pinning the validate half of R4 (architecture §7).
    #[test]
    fn golden_validates_against_validate_schema() {
        let validator = validate_validator();
        let golden = golden_value();
        if let Err(error) = validator.validate(&golden) {
            panic!("the golden validate report must validate against validate.schema.json: {error}");
        }
    }

    /// Golden snapshot test. `validate_json` of the valid fixture, parsed to a `Value`,
    /// equals the committed golden parsed to a `Value` (compared as parsed JSON so
    /// whitespace/trailing-newline differences are not brittle while every key/value is
    /// pinned). This is the snapshot that locks the report wire shape to a committed
    /// artifact.
    #[test]
    fn validate_json_equals_committed_golden() {
        let produced: Value = serde_json::from_str(
            &validate_json(conformance("valid/minimal")).expect("validate_json succeeds"),
        )
        .expect("validate output is valid JSON");

        assert_eq!(
            produced,
            golden_value(),
            "validate of the valid fixture must equal the committed golden \
             (regenerate the golden only on a format_version bump — see conformance/README.md)"
        );
    }

    /// Report-states-which-ran (FOLD honesty / spec §14 note). The golden's `checks`
    /// array contains **all 20** §14 ids; the v0.1 honest skips (M6 regularity leg, L3
    /// absence-vs-NaN leg, T2 cross-artifact axis leg) appear with `status:"skipped"` +
    /// a non-empty `detail`; every other check is `status:"ran"` with `result:"pass"`;
    /// top-level `conformant:true`. This pins, in the committed artifact, that the report
    /// clearly reports which checks ran vs were skipped.
    #[test]
    fn golden_clearly_reports_which_checks_ran_vs_skipped() {
        let golden = golden_value();
        let checks = golden
            .get("checks")
            .and_then(Value::as_array)
            .expect("golden checks array");

        // All 20 §14 ids are present, in spec order.
        let ids: Vec<&str> = checks
            .iter()
            .map(|c| c.get("id").and_then(Value::as_str).expect("check id"))
            .collect();
        let expected_ids = [
            "M1", "M2", "M3", "M4", "M5", "M6", "L1", "L2", "L3", "I1", "I2", "I3", "H1", "H2",
            "T1", "T2", "G1", "G2", "G3", "Geo1",
        ];
        assert_eq!(ids, expected_ids, "the golden lists every §14 id in spec order");

        // The v0.1 honest skips: each is status:skipped with a non-empty detail reason
        // and result:null (a skip never carries a verdict, the §14-note honesty rule).
        let skipped: BTreeSet<&str> = ["M6", "L3", "T2"].into_iter().collect();
        for check in checks {
            let id = check.get("id").and_then(Value::as_str).expect("id");
            let status = check.get("status").and_then(Value::as_str).expect("status");
            if skipped.contains(id) {
                assert_eq!(status, "skipped", "{id} is an honest v0.1 skip");
                assert!(
                    check.get("result").expect("result key").is_null(),
                    "{id} skip carries no result"
                );
                let detail = check.get("detail").and_then(Value::as_str);
                assert!(
                    detail.is_some_and(|d| !d.is_empty()),
                    "{id} skip carries a non-empty reason (the §14 note)"
                );
            } else {
                // Every other check ran and passed on the valid fixture.
                assert_eq!(status, "ran", "{id} ran on the valid fixture");
                assert_eq!(
                    check.get("result").and_then(Value::as_str),
                    Some("pass"),
                    "{id} passed on the valid fixture"
                );
            }
        }

        // Top-level verdict.
        assert_eq!(
            golden.get("conformant").and_then(Value::as_bool),
            Some(true),
            "the valid fixture is conformant"
        );
    }

    /// Negative schema test. A golden mutated with (a) an injected extra top-level key,
    /// (b) a check object missing its `id`, and (c) a check object with an unknown
    /// `status` each **fails** schema validation (`additionalProperties:false` /
    /// `required` / enum constraints), proving the schema catches a shape drift.
    #[test]
    fn mutated_golden_with_drift_is_rejected_by_schema() {
        let validator = validate_validator();

        // Sanity: the unmutated golden validates.
        assert!(
            validator.is_valid(&golden_value()),
            "the unmutated golden must validate"
        );

        // (a) An arbitrary injected extra top-level key.
        let mut with_extra = golden_value();
        with_extra
            .as_object_mut()
            .expect("golden object")
            .insert("schema_version".to_string(), Value::from("9.9"));
        assert!(
            !validator.is_valid(&with_extra),
            "an extra top-level key must be rejected (additionalProperties:false catches drift)"
        );

        // (b) A check object missing its required `id`.
        let mut missing_id = golden_value();
        missing_id
            .get_mut("checks")
            .and_then(Value::as_array_mut)
            .expect("checks array")[0]
            .as_object_mut()
            .expect("check object")
            .remove("id");
        assert!(
            !validator.is_valid(&missing_id),
            "a check missing `id` must be rejected (required catches drift)"
        );

        // (c) A check object with an unknown `status`.
        let mut bad_status = golden_value();
        bad_status
            .get_mut("checks")
            .and_then(Value::as_array_mut)
            .expect("checks array")[0]
            .as_object_mut()
            .expect("check object")
            .insert("status".to_string(), Value::from("maybe"));
        assert!(
            !validator.is_valid(&bad_status),
            "an unknown `status` must be rejected (enum catches drift)"
        );
    }

    /// Both invalids' reports serialize over the wire (smoke test, spec §14). Each MS2
    /// invalid's report is produced as valid JSON: the `missing-root-rollup` tree
    /// serializes with `conformant:false` and `L1` `result:"fail"` (the wire shape
    /// carries a fail correctly); the `wrong-format-version` tree is an entry-gate `Err`
    /// (the §0 hard cut), so `validate_json` surfaces it as a `ValidateError`, never a
    /// `conformant:true` report. The exhaustive per-check golden matrix is MS8.
    #[test]
    fn both_invalids_reports_serialize_carrying_the_verdict() {
        // missing-root-rollup → a conformant:false report with L1 result:fail, valid JSON.
        let json = validate_json(conformance("invalid/missing-root-rollup"))
            .expect("missing-root-rollup serializes (the gap is a check fail, not an Err)");
        let value: Value = serde_json::from_str(&json).expect("output is valid JSON");

        // It validates against the schema (the wire shape carries a fail correctly).
        let validator = validate_validator();
        assert!(
            validator.is_valid(&value),
            "a non-conformant report still matches the pinned wire shape"
        );

        assert_eq!(
            value.get("conformant").and_then(Value::as_bool),
            Some(false),
            "missing-root-rollup is non-conformant"
        );
        let l1 = value
            .get("checks")
            .and_then(Value::as_array)
            .expect("checks array")
            .iter()
            .find(|c| c.get("id").and_then(Value::as_str) == Some("L1"))
            .expect("L1 present");
        assert_eq!(l1.get("status").and_then(Value::as_str), Some("ran"), "L1 ran");
        assert_eq!(
            l1.get("result").and_then(Value::as_str),
            Some("fail"),
            "L1 carries the fail verdict on the wire"
        );

        // wrong-format-version → the §0 hard cut wins: an entry-gate Err, never a report.
        match validate_json(conformance("invalid/wrong-format-version")) {
            Err(ValidateError::Manifest(CoreError::UnknownFormatVersion { found })) => {
                assert_eq!(found, "0.2");
            }
            other => panic!("expected the §0 hard cut Err, got {other:?}"),
        }
    }

    // --- MS8-S3: Bucket-B one-violation invalids (I1/I2/I3/H1/T1/L2) ------------------
    //
    // Each fixture is one surgical mutation off the valid baseline (LOW-2; the
    // generator's `assert_differs_in_exactly_one_way` proves the one-mutation invariant
    // at generation time). Its negative is a CLEAN `conformant:false` report with exactly
    // ONE §14 check `ran:fail` and every other check `pass`-or-`skip`. Each test pins the
    // exact failing id, runs the purity assertion (no second check trips), and snapshots
    // the report against the committed per-fixture golden under
    // `conformance/goldens/invalid-<name>.validate.json`.

    /// Reads a committed per-fixture golden validate report as a parsed `Value`.
    ///
    /// Maps a fixture path (e.g. `"invalid/non-monotonic-time"`) to the relocated golden
    /// under `conformance/goldens/`, flattening `/` → `-` and appending `.validate.json`
    /// (so `"invalid/non-monotonic-time"` reads
    /// `conformance/goldens/invalid-non-monotonic-time.validate.json`). The golden lives
    /// OUTSIDE the gitignored fixture trees so `regenerate.sh` never clobbers it.
    ///
    /// Regeneration workflow (when the report shape legitimately changes — a
    /// `format_version` bump only): run `validate_json(conformance("invalid/<name>"))`,
    /// pretty-print it, and overwrite
    /// `conformance/goldens/invalid-<name>.validate.json`. See `conformance/README.md`.
    /// Never hand-edited.
    fn fixture_golden_value(name: &str) -> Value {
        let flattened = name.replace('/', "-");
        let path = conformance("goldens").join(format!("{flattened}.validate.json"));
        let raw = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        serde_json::from_str(&raw).expect("the golden must be valid JSON")
    }

    /// Asserts a Bucket-B fixture pins **exactly** one §14 check `ran:fail`.
    ///
    /// Shared `(fixture, pinned_id)` helper: validates the fixture (which discovers — the
    /// violation is a check fail, not an `Err`), asserts `conformant:false`, the pinned id
    /// is `status:ran result:fail`, **every other** check has `result != fail` (the purity
    /// assertion that guards a second check from tripping), and the produced report is
    /// snapshot-equal to the committed per-fixture golden.
    fn assert_pins_exactly(name: &str, pinned: CheckId) {
        let report = validate(conformance(name))
            .unwrap_or_else(|e| panic!("{name} must discover (the gap is a check fail, not an Err): {e:?}"));

        // The pinned check ran and FAILED.
        let outcome = report
            .find(pinned)
            .unwrap_or_else(|| panic!("{name}: {pinned:?} present in the report"));
        assert_eq!(outcome.status(), CheckStatus::Ran, "{name}: {pinned:?} ran");
        assert_eq!(
            outcome.result(),
            Some(CheckResult::Fail),
            "{name}: {pinned:?} must fail"
        );

        // Non-conformant (fail-closed on the one violated MUST that ran).
        assert!(!report.conformant(), "{name} ⇒ non-conformant");

        // PURITY: EXACTLY the pinned id fails; every other check is pass-or-skip.
        for other in report.checks() {
            if other.id() == pinned {
                continue;
            }
            assert_ne!(
                other.result(),
                Some(CheckResult::Fail),
                "{name}: only {pinned:?} may fail; {:?} also failed",
                other.id()
            );
        }

        // SNAPSHOT: the produced report equals the committed per-fixture golden.
        let produced: Value = serde_json::from_str(
            &validate_json(conformance(name)).expect("validate_json succeeds"),
        )
        .expect("validate output is valid JSON");
        assert_eq!(
            produced,
            fixture_golden_value(name),
            "{name}: validate must equal the committed golden \
             (regenerate the golden only on a format_version bump — see conformance/README.md)"
        );

        // The golden also matches the pinned wire shape (R4 schema lock).
        let validator = validate_validator();
        assert!(
            validator.is_valid(&fixture_golden_value(name)),
            "{name}: the golden must validate against validate.schema.json"
        );
    }

    #[test]
    fn i1_missing_basin_id_pins_exactly_i1() {
        // One basin's scalar_dynamic dropped its basin_id column (spec §3 / I1). The
        // reader records has_basin_id=false (it does NOT error), so check_i1 ⇒ ran:fail.
        // I3/I2/H1 are unaffected (the None basin is filtered/skipped; basin_id is never
        // a catalogued field) — confirmed I1-only.
        assert_pins_exactly("invalid/missing-basin-id-column", CheckId::I1);
    }

    #[test]
    fn i2_folder_mismatch_pins_exactly_i2() {
        // One basin's in-file basin_id is rewritten to a unique foreign value (9999) that
        // disagrees with its basin=<id> folder (spec §3 / I2) ⇒ check_i2 ran:fail. I3
        // stays pass (the value is kept unique); I1 stays pass (the column is present).
        assert_pins_exactly("invalid/basin-id-folder-mismatch", CheckId::I2);
    }

    #[test]
    fn h1_ragged_schema_pins_exactly_h1() {
        // One basin's scalar_dynamic renames its data field streamflow→flow (spec §5 /
        // H1); only the name diverges (dtype/quadrant kept) ⇒ check_h1 ran:fail. T1/I1/I2/
        // I3 stay pass (time/basin_id unchanged) — confirmed H1-only.
        assert_pins_exactly("invalid/ragged-field-schema", CheckId::H1);
    }

    #[test]
    fn t1_non_monotonic_pins_exactly_t1() {
        // One basin's scalar_dynamic time is written descending across row groups (spec
        // §6.3 / T1) ⇒ time_sorted_ascending false ⇒ check_t1 ran:fail. The single pinned
        // T1 mutation (the nullable/mistyped/misnamed legs are covered in-memory by
        // t1_negative_per_leg).
        assert_pins_exactly("invalid/non-monotonic-time", CheckId::T1);
    }

    #[test]
    fn l2_missing_gridded_dynamic_pins_exactly_l2() {
        // One basin's gridded_dynamic/ subtree is deleted (spec §4 / L2).
        // declares_gridded_dynamic stays true dataset-wide, so that basin's empty
        // dynamic_artifacts() ⇒ check_l2 ran:fail. H2 does NOT co-fail: the surviving COG
        // keeps the era5 static label, so every basin's label set is {era5} — confirmed
        // L2-only (the H2-collision caveat held).
        assert_pins_exactly("invalid/missing-gridded-dynamic-subtree", CheckId::L2);
    }

    // --- MS8-S2: georef / grid-label one-violation invalids (M5/G2/H2) ----------------
    //
    // Each is one surgical mutation off the valid baseline (the generator's
    // `assert_differs_in_exactly_one_way` proves the one-mutation invariant at generation
    // time) and produces a CLEAN `conformant:false` report with exactly ONE §14 check
    // `ran:fail`. The purity legs were confirmed empirically before committing (see each
    // test's note); `assert_pins_exactly` re-checks purity + snapshots against the golden.

    #[test]
    fn crs_mismatch_pins_exactly_m5() {
        // The manifest crs is rewritten to a different valid EPSG (EPSG:3857) while every
        // file keeps EPSG:4326 (spec §7/§11 / M5). check_m5 compares each GridInfo.crs()
        // against the manifest crs and ran:fails on the first grid. M4 stays pass (crs is
        // a non-empty string), M6 stays skip, Geo1/G3 stay pass — confirmed M5-only.
        assert_pins_exactly("invalid/crs-mismatch", CheckId::M5);
    }

    #[test]
    fn misaligned_shared_label_pins_exactly_g2() {
        // One basin's gridded_static COG is re-emitted under the SAME era5 label at a
        // half-cell-shifted geometry, its Zarr left at the baseline (spec §8 / G2). The
        // shared era5 label now appears in both subtrees but their extents diverge, so
        // check_g2 ran:fails. H2 stays pass (label set still {era5}) and G3 stays pass
        // (georef intact) — confirmed G2-only.
        assert_pins_exactly("invalid/misaligned-shared-label", CheckId::G2);
    }

    #[test]
    fn divergent_grid_label_set_pins_exactly_h2() {
        // One basin's COG+Zarr are re-emitted under a divergent era5b label (era5.* →
        // era5b.*) so that basin's grid-label set is {era5b} while every other basin's is
        // {era5} (spec §8 / H2) ⇒ check_h2 ran:fails. Renaming BOTH subtrees keeps the
        // shared era5b label coinciding, so G2 stays pass — confirmed H2-only.
        assert_pins_exactly("invalid/divergent-grid-label-set", CheckId::H2);
    }

    /// The DTO is the single wire-shape surface (the inert types stay serde-free).
    /// A produced report serializes to exactly `{checks, conformant}` at the top level
    /// and each check entry is exactly `{id, status, result, depth, detail}` — no
    /// inert-violating / derived key. Pins the inert/agnostic shape (spec §1) directly.
    #[test]
    fn report_dto_top_level_and_check_key_sets_are_exact() {
        let json = validate_json(conformance("valid/minimal")).expect("validate_json succeeds");
        let value: Value = serde_json::from_str(&json).expect("valid JSON");

        let top_keys: BTreeSet<String> = value
            .as_object()
            .expect("top-level object")
            .keys()
            .cloned()
            .collect();
        let expected_top: BTreeSet<String> = ["checks", "conformant"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(top_keys, expected_top, "top level is exactly {{checks, conformant}}");

        let check_keys: BTreeSet<String> = ["id", "status", "result", "depth", "detail"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        for check in value
            .get("checks")
            .and_then(Value::as_array)
            .expect("checks array")
        {
            let keys: BTreeSet<String> = check
                .as_object()
                .expect("check object")
                .keys()
                .cloned()
                .collect();
            assert_eq!(
                keys, check_keys,
                "a check is exactly {{id, status, result, depth, detail}} (inert)"
            );
        }
    }
}
